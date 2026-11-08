//! `AIEX` v28 road network of the first game's city file: road polylines, lane links with their connections, junction gates and the spatial grid.
//! ORIGIN NOTICE: this file was brought into this repository from the in-development "game engine Rust Rework research"
//! project. It was not written for this repository. Differences from that version:
//! - not itemised for this file
//!
//! Layout notes: re/formats/TRAFEGO_AIEX.md / re/formats/D3C.md in the reverse engineering docs. All pointers in the
//! payload are relative.
//!
//! The payload is found inside `GERD` in the global `GESR` section of a `.d3c` (see `load`).
//! Exposes `Aiex::parse`, `load`, and the types `Aiex`, `Piece`, `Segment`, `Link`, `Node`, `Rec` and `LanePath`.
//! Uses `chnk` to find the `GESR`, `GERD` and `AIEX` entries. Used by `zartex-app`'s city view (`city3d.rs`), which calls
//! `formats::aiex::load`.

use crate::chnk::Chunk;

const NIL: [i32; 2] = [0, 0x7FFF_FFF8];

/// A polyline of the road: its start point, then one point per step (see `Aiex::parse`).
#[derive(Debug, Clone)]
pub struct Piece {
    /// Polyline points (start + each step).
    pub points: Vec<[f32; 3]>,
}

/// One road segment: a 28-byte record with its length, the number of lanes on each side, the first of its polylines, the
/// number of polylines, and its flag byte.
#[derive(Debug, Clone)]
pub struct Segment {
    pub length: f32,
    /// Lanes on side 0 (+x) and side 1 (-x).
    pub lanes: [u8; 2],
    pub first_piece: usize,
    pub n_pieces: usize,
    /// Bit 0 of the segment flag byte: fast road (27 m/s cap instead of 18).
    pub fast: bool,
    /// The whole byte at +27 of the 28-byte segment record (the game engine Rust Rework research reads only bit 0) and the word at +24 (low byte =
    /// piece count): kept to list the segments (`inspect --aiex-segments`).
    pub flags: u8,
    pub word24: u32,
}

/// One lane link: a 16-byte record with the segment and lane profile it uses, its junction nodes, and its neighbouring
/// links at each extremity.
#[derive(Debug, Clone, Default)]
pub struct Link {
    /// Segment (rec44) that owns the lane.
    pub seg: u16,
    /// Profile index (lateral offset of the lane line).
    pub profile: u16,
    /// Junction nodes of this link (branching options).
    pub nodes: Vec<u16>,
    /// Neighbours at extremity 0 and 1: (link, extremity of the neighbour).
    pub neighbors: [Vec<(u16, u8)>; 2],
}

/// A junction gate: its two gate points, the target link, the owning link, and the 32-bit flag word `w2`.
#[derive(Debug, Clone, Copy)]
pub struct Node {
    pub gate: [[f32; 3]; 2],
    /// Target link.
    pub target: u16,
    pub owner: u16,
    pub w2: u32,
}

impl Node {
    /// Entry extremity on the target lane (0 = enters at its start).
    pub fn entry(&self) -> u8 {
        ((self.w2 >> 24) & 1) as u8
    }
    /// True when bit 23 of `w2` is set.
    pub fn is_curve(&self) -> bool {
        (self.w2 >> 16) & 0x80 != 0
    }
    /// True when bit 31 of `w2` is set.
    pub fn atypical(&self) -> bool {
        (self.w2 >> 24) & 0x80 != 0
    }
}

/// One record of the road graph: a 44-byte record with its segment, four lists of lane links, and its neighbouring records.
#[derive(Debug, Clone, Default)]
pub struct Rec {
    pub seg: u16,
    /// Lane links per list: 0 = side 0 lanes, 1 = side 1 lanes, 2/3 = other.
    pub lists: [Vec<u16>; 4],
    /// Neighbouring records at extremity 0 and 1 of the segment: (record, extremity of the neighbour by which it is
    /// entered). Pointers at +0 / +4, counts at +8 / +9, entries `u16` with bit 15 = the entry extremity; the index is
    /// into this record table (`FUN_00435220`: `(u16 & 0x7FFF) * 0x2C + records`). The graph is undirected: in Miami all
    /// 1524 entries are reciprocal. The police trail search walks it (`FUN_0045C590`). [exe 0x00435220]
    pub neighbors: [Vec<(u16, u8)>; 2],
}

/// A parsed `AIEX` payload: segments, polylines, lane profiles, lane links, junction nodes, graph records, the owner of
/// each link, and the 240 m lookup grid.
#[derive(Debug)]
pub struct Aiex {
    pub segments: Vec<Segment>,
    pub pieces: Vec<Piece>,
    pub profile_x: Vec<f32>,
    pub profile_y: Vec<f32>,
    pub links: Vec<Link>,
    pub nodes: Vec<Node>,
    pub recs: Vec<Rec>,
    /// link -> (rec index, list 0..3, index inside the list)
    pub link_owner: Vec<Option<(u16, u8, u16)>>,
    pub cell_min: (i32, i32),
    pub grid_wh: (i32, i32),
    pub grid1: Vec<Vec<u16>>,
}

/// Bounds-checked reader over the payload. Every read returns `None` past the end.
struct P<'a>(&'a [u8]);

impl P<'_> {
    fn u32(&self, o: usize) -> Option<u32> {
        self.0.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
    }
    fn i32(&self, o: usize) -> Option<i32> {
        self.u32(o).map(|v| v as i32)
    }
    fn u16(&self, o: usize) -> Option<u16> {
        self.0.get(o..o + 2).map(|s| u16::from_le_bytes(s.try_into().unwrap()))
    }
    fn u8(&self, o: usize) -> Option<u8> {
        self.0.get(o).copied()
    }
    fn f32(&self, o: usize) -> Option<f32> {
        self.u32(o).map(f32::from_bits)
    }
    /// Position of a relative pointer: the `i32` stored at `o`, added to `o`. `None` when negative or unreadable.
    fn rel(&self, o: usize) -> Option<usize> {
        usize::try_from(o as i64 + self.i32(o)? as i64).ok()
    }
}

impl Aiex {
    /// Parses an `AIEX` payload. The header gives, for each table, its count and an offset relative to the header. Returns
    /// `None` when a read goes past the end of the payload.
    pub fn parse(data: &[u8]) -> Option<Aiex> {
        let p = P(data);
        let n_ptr = p.u32(8)? as usize;
        let o_ptr = p.u32(0xC)? as usize + 8;
        let n_pieces = p.u32(0x10)? as usize;
        let o_pieces = p.u32(0x14)? as usize + 0x10;
        let n_seg = p.u32(0x18)? as usize;
        let o_seg = p.u32(0x1C)? as usize + 0x18;
        let n_prof = p.u32(0x20)? as usize;
        let o_prof = p.u32(0x24)? as usize + 0x20;
        let n_node = p.u32(0x40)? as usize;
        let o_node = p.u32(0x44)? as usize + 0x40;
        let n_rec = p.u32(0x48)? as usize;
        let o_rec = p.u32(0x4C)? as usize + 0x48;
        let n_link = p.u32(0x50)? as usize;
        let o_link = p.u32(0x54)? as usize + 0x50;
        let cell_min = (p.i32(0x58)?, p.i32(0x5C)?);
        let grid_wh = (p.i32(0x60)?, p.i32(0x64)?);
        let n_grid1 = p.u32(0x68)? as usize;
        let o_grid1 = p.u32(0x6C)? as usize + 0x68;
        let o_tail = p.u32(0x78)? as usize + 0x78;

        let mut profile_x = Vec::with_capacity(n_prof);
        let mut profile_y = Vec::with_capacity(n_prof);
        for i in 0..n_prof {
            profile_x.push(p.f32(o_prof + 12 * i)?);
            profile_y.push(p.f32(o_prof + 12 * i + 4)?);
        }
        let _ = (n_ptr, o_ptr);

        // each piece is a start point followed by `n` steps of 16 bytes: a direction (three floats) and a length
        let mut pieces = Vec::with_capacity(n_pieces);
        for i in 0..n_pieces {
            let o = o_pieces + 48 * i;
            let n = p.u32(o + 0x14)? as usize;
            let tgt = usize::try_from((o + 0x14) as i64 + p.i32(o + 0x18)? as i64).ok()?;
            let mut pt = [p.f32(o)?, p.f32(o + 4)?, p.f32(o + 8)?];
            let mut points = vec![pt];
            for k in 0..n {
                let s = tgt + 16 * k;
                let len = p.f32(s + 12)?;
                pt = [pt[0] + p.f32(s)? * len, pt[1] + p.f32(s + 4)? * len, pt[2] + p.f32(s + 8)? * len];
                points.push(pt);
            }
            pieces.push(Piece { points });
        }

        let mut segments = Vec::with_capacity(n_seg);
        for i in 0..n_seg {
            let o = o_seg + 28 * i;
            segments.push(Segment {
                length: p.f32(o)?,
                lanes: [p.u8(o + 12)?, p.u8(o + 13)?],
                first_piece: p.u16(o + 14)? as usize,
                n_pieces: (p.u32(o + 24)? & 0xFF) as usize,
                fast: p.u8(o + 27)? & 1 != 0,
                flags: p.u8(o + 27)?,
                word24: p.u32(o + 24)?,
            });
        }

        let mut links = vec![Link::default(); n_link];
        for (i, l) in links.iter_mut().enumerate() {
            let o = o_link + 16 * i;
            l.profile = p.u16(o + 10)?;
            let v = p.u16(o + 12)? as usize;
            let e = p.u16(o + 14)?;
            l.seg = e & 0x3FF;
            let n_nodes = (e >> 10) as usize & 7;
            for j in 0..n_nodes {
                l.nodes.push(p.u16(o_tail + 2 * (v + j))?);
            }
            for ext in 0..2 {
                let cnt = p.u8(o + 8 + ext)? as usize;
                if cnt > 0 {
                    let q = p.rel(o + 4 * ext)?;
                    for j in 0..cnt {
                        let w = p.u16(q + 2 * j)?;
                        l.neighbors[ext].push((w & 0x7FFF, (w >> 15) as u8));
                    }
                }
            }
        }

        let mut nodes = Vec::with_capacity(n_node);
        for i in 0..n_node {
            let o = o_node + 32 * i;
            nodes.push(Node {
                gate: [[p.f32(o)?, p.f32(o + 4)?, p.f32(o + 8)?], [p.f32(o + 16)?, p.f32(o + 20)?, p.f32(o + 24)?]],
                target: p.u16(o + 12)?,
                owner: p.u16(o + 14)?,
                w2: p.u32(o + 28)?,
            });
        }

        let mut recs = Vec::with_capacity(n_rec);
        let mut link_owner = vec![None; n_link];
        for i in 0..n_rec {
            let o = o_rec + 44 * i;
            let mut r = Rec { seg: p.u16(o + 10)?, ..Default::default() };
            for ext in 0..2 {
                let cnt = p.u8(o + 8 + ext)? as usize;
                if cnt > 0 {
                    let q = p.rel(o + 4 * ext)?;
                    for j in 0..cnt {
                        let w = p.u16(q + 2 * j)?;
                        let rec = (w & 0x7FFF) as usize;
                        if rec < n_rec {
                            r.neighbors[ext].push((rec as u16, (w >> 15) as u8));
                        }
                    }
                }
            }
            for k in 0..4 {
                let co = o + 0xC + 8 * k;
                let cnt = p.u32(co)? as usize;
                if cnt == 0 {
                    continue;
                }
                let base = p.rel(co + 4).map(|v| v).and_then(|_| usize::try_from(co as i64 + p.i32(co + 4)? as i64).ok())?;
                for j in 0..cnt {
                    let at = base + 4 * j;
                    let v = p.i32(at)?;
                    if NIL.contains(&v) {
                        continue;
                    }
                    let link = ((at as i64 + v as i64 - o_link as i64) / 16) as usize;
                    if link < n_link {
                        r.lists[k].push(link as u16);
                        if link_owner[link].is_none() {
                            link_owner[link] = Some((i as u16, k as u8, (r.lists[k].len() - 1) as u16));
                        }
                    }
                }
            }
            recs.push(r);
        }

        let mut grid1 = Vec::with_capacity(n_grid1);
        for i in 0..n_grid1 {
            let co = o_grid1 + 8 * i;
            let cnt = p.u32(co)? as usize;
            let mut cell = Vec::new();
            if cnt > 0 {
                let base = usize::try_from(co as i64 + p.i32(co + 4)? as i64).ok()?;
                for k in 0..cnt {
                    let at = base + 4 * k;
                    let v = p.i32(at)?;
                    if NIL.contains(&v) {
                        continue;
                    }
                    let rec = ((at as i64 + v as i64 - o_rec as i64) / 44) as usize;
                    if rec < n_rec {
                        cell.push(rec as u16);
                    }
                }
            }
            grid1.push(cell);
        }

        Some(Aiex { segments, pieces, profile_x, profile_y, links, nodes, recs, link_owner, cell_min, grid_wh, grid1 })
    }

    // ---- queries on the record graph
    /// Height range `(min, mean, max)` of the polylines of record `rec`'s segment; `None` without points.
    pub fn rec_heights(&self, rec: usize) -> Option<(f32, f32, f32)> {
        let seg = self.segments.get(self.recs.get(rec)?.seg as usize)?;
        let (mut lo, mut hi, mut sum, mut n) = (f32::MAX, f32::MIN, 0.0f32, 0usize);
        for piece in self.pieces.get(seg.first_piece..seg.first_piece + seg.n_pieces)? {
            for p in &piece.points {
                lo = lo.min(p[1]);
                hi = hi.max(p[1]);
                sum += p[1];
                n += 1;
            }
        }
        (n > 0).then(|| (lo, sum / n as f32, hi))
    }

    /// The connected components of the record graph (neighbour lists of both extremities, undirected), each as a sorted
    /// list of records, in order of their lowest record.
    pub fn rec_components(&self) -> Vec<Vec<usize>> {
        let n = self.recs.len();
        let mut seen = vec![false; n];
        let mut out = Vec::new();
        for start in 0..n {
            if seen[start] {
                continue;
            }
            let mut comp = Vec::new();
            let mut stack = vec![start];
            seen[start] = true;
            while let Some(r) = stack.pop() {
                comp.push(r);
                for &(nb, _) in self.recs[r].neighbors.iter().flatten() {
                    let nb = nb as usize;
                    if nb < n && !seen[nb] {
                        seen[nb] = true;
                        stack.push(nb);
                    }
                }
            }
            comp.sort_unstable();
            out.push(comp);
        }
        out
    }

    /// The components of the record graph that have lane points and lie wholly above `min_y` metres: a road network
    /// cut off from the ground, with no ramp to it. In Miami the MetroMover's track is expected to be one of them
    /// [hipótese] (the viaducts, 10.5..11 m, join the streets by ramps and so belong to the ground's component).
    pub fn isolated_elevated(&self, min_y: f32) -> Vec<Vec<usize>> {
        self.rec_components()
            .into_iter()
            .filter(|c| {
                let lanes = c.iter().any(|&r| self.recs[r].lists.iter().any(|l| !l.is_empty()));
                lanes && c.iter().all(|&r| self.rec_heights(r).is_some_and(|h| h.0 > min_y))
            })
            .collect()
    }

    /// Every record of `comp` has neighbours at both extremities: the component has no dead end, as a circuit.
    pub fn is_closed(&self, comp: &[usize]) -> bool {
        comp.iter().all(|&r| self.recs[r].neighbors.iter().all(|n| !n.is_empty()))
    }

    /// Takes the records out of use: no lanes, no neighbours. Their segments stay in the tables.
    pub fn disable_recs(&mut self, recs: &[usize]) {
        for &r in recs {
            let rec = &mut self.recs[r];
            rec.lists = Default::default();
            rec.neighbors = Default::default();
        }
    }

    /// Records of the 240 m grid cell that contains (x, z), or an empty slice outside the grid.
    pub fn cell_recs(&self, x: f32, z: f32) -> &[u16] {
        let cx = ((x - 120.0) / 240.0).floor() as i32 - self.cell_min.0;
        let cz = ((z - 120.0) / 240.0).floor() as i32 - self.cell_min.1;
        if cx < 0 || cz < 0 || cx >= self.grid_wh.0 || cz >= self.grid_wh.1 {
            return &[];
        }
        self.grid1.get((cz * self.grid_wh.0 + cx) as usize).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Driving side of a lane link: 0 = along the polyline (extremity 0 -> 1),
    /// 1 = against it. Only lists 0 and 1 are traffic lanes.
    pub fn lane_side(&self, link: usize) -> Option<u8> {
        match self.link_owner.get(link).copied().flatten() {
            Some((_, k, _)) if k < 2 => Some(k),
            _ => None,
        }
    }

    /// Centre line of a segment: all its pieces joined. A first point of a piece that lies within 0.05 m of the last point
    /// is not repeated.
    pub fn segment_polyline(&self, seg: usize) -> Vec<[f32; 3]> {
        let s = &self.segments[seg];
        let mut out: Vec<[f32; 3]> = Vec::new();
        for pc in s.first_piece..s.first_piece + s.n_pieces {
            let Some(piece) = self.pieces.get(pc) else { continue };
            for (i, p) in piece.points.iter().enumerate() {
                if i == 0 {
                    if let Some(last) = out.last() {
                        let d = (last[0] - p[0]).hypot(last[2] - p[2]);
                        if d < 0.05 {
                            continue;
                        }
                    }
                }
                out.push(*p);
            }
        }
        out
    }

    /// The lane's driving path in travel order, offset from the road centre
    /// line by the lane's lateral position (right-hand traffic).
    pub fn lane_path(&self, link: usize) -> Option<LanePath> {
        let (rec, side, _) = self.link_owner.get(link).copied().flatten()?;
        if side > 1 {
            return None;
        }
        let seg = self.recs[rec as usize].seg as usize;
        let center = self.segment_polyline(seg);
        if center.len() < 2 {
            return None;
        }
        let x = *self.profile_x.get(self.links[link].profile as usize)?;
        let mut pts: Vec<[f32; 3]> = Vec::with_capacity(center.len());
        let mut steps: Vec<f32> = Vec::with_capacity(center.len());
        for i in 0..center.len() {
            let a = center[i.saturating_sub(1)];
            let b = center[(i + 1).min(center.len() - 1)];
            let (tx, tz) = (b[0] - a[0], b[2] - a[2]);
            let l = tx.hypot(tz).max(1e-6);
            let (rx, rz) = (-tz / l, tx / l);
            pts.push([center[i][0] + x * rx, center[i][1], center[i][2] + x * rz]);
            let nxt = center[(i + 1).min(center.len() - 1)];
            steps.push((nxt[0] - center[i][0]).hypot(nxt[2] - center[i][2]));
        }
        // a lane on side 1 runs against the polyline: reverse the points, and move each step to the point it starts from
        if side == 1 {
            pts.reverse();
            // the step length that follows each point in travel order
            let mut s2 = vec![0.0; steps.len()];
            for i in 1..steps.len() {
                s2[steps.len() - 1 - i] = steps[i - 1];
            }
            s2[steps.len() - 1] = *steps.first().unwrap_or(&0.0);
            steps = s2;
        }
        Some(LanePath { points: pts, steps, side, seg, fast: self.segments[seg].fast })
    }
}

/// A lane's driving path: its points in travel order, the length of the step at each point, its side (0 or 1), its
/// segment index, and whether the segment is a fast road.
#[derive(Debug, Clone)]
pub struct LanePath {
    pub points: Vec<[f32; 3]>,
    /// Length of the original polyline step that starts at each point
    /// (drives the target speed).
    pub steps: Vec<f32>,
    pub side: u8,
    pub seg: usize,
    pub fast: bool,
}

/// The `AIEX` payload of a `.d3c`, if present.
pub fn load(root: &Chunk) -> Option<Aiex> {
    let g = root.entries.iter().find(|e| e.tag() == "GESR" && e.is_chnk())?;
    let kids = g.children().ok()?;
    let gerd = kids.find("GERD").next()?;
    let inner = gerd.children().ok()?;
    let a = inner.find("AIEX").next()?;
    Aiex::parse(a.data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny network: records 0-1 on the ground joined to the viaduct 2 by neighbour lists, and an elevated loop 3-4-5
    /// with lanes and no way down.
    fn network() -> Aiex {
        let seg = |first| Segment { length: 10.0, lanes: [1, 1], first_piece: first, n_pieces: 1, fast: false, flags: 0, word24: 1 };
        let piece = |y: f32| Piece { points: vec![[0.0, y, 0.0], [10.0, y, 0.0]] };
        let ys = [0.0, 1.0, 10.5, 9.0, 9.0, 9.0];
        let mut recs: Vec<Rec> = (0..6).map(|i| Rec { seg: i as u16, ..Default::default() }).collect();
        for r in recs.iter_mut() {
            r.lists[0].push(0);
        }
        let join = |recs: &mut Vec<Rec>, a: usize, b: usize| {
            recs[a].neighbors[1].push((b as u16, 0));
            recs[b].neighbors[0].push((a as u16, 1));
        };
        join(&mut recs, 0, 1);
        join(&mut recs, 1, 2);
        join(&mut recs, 3, 4);
        join(&mut recs, 4, 5);
        join(&mut recs, 5, 3);
        Aiex {
            segments: (0..6).map(seg).collect(),
            pieces: ys.iter().map(|&y| piece(y)).collect(),
            profile_x: Vec::new(),
            profile_y: Vec::new(),
            links: Vec::new(),
            nodes: Vec::new(),
            recs,
            link_owner: Vec::new(),
            cell_min: (0, 0),
            grid_wh: (0, 0),
            grid1: Vec::new(),
        }
    }

    /// The viaduct joined to the street stays; the elevated loop with no ramp down is the isolated component.
    #[test]
    fn an_elevated_loop_with_no_ramp_is_isolated() {
        let mut a = network();
        assert_eq!(a.rec_components(), vec![vec![0, 1, 2], vec![3, 4, 5]]);
        assert_eq!(a.isolated_elevated(6.0), vec![vec![3, 4, 5]]);
        assert!(a.is_closed(&[3, 4, 5]) && !a.is_closed(&[0, 1, 2]));
        assert!(a.isolated_elevated(9.5).is_empty());
        a.disable_recs(&[3, 4, 5]);
        assert!(a.recs[3].lists.iter().all(|l| l.is_empty()) && a.recs[4].neighbors.iter().all(|n| n.is_empty()));
        assert!(!a.recs[1].neighbors[1].is_empty());
    }
}
