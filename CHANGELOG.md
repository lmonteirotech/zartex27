# Changelog

All notable changes are listed here. The version number is `major.minor.patch` (semantic versioning).
Releases are made by hand from the "Release" workflow; they are not made by pull requests or merges.

## 0.1.0 - first release

- Mission editor for the first game and for Driver: Parallel Lines: logic graph, inspector, wire editing, creation and deletion of actors and nodes.
- Lua mission API (Lua 5.2) with 134 functions and 84 constants, a command-line compiler (`zartex-luac`) and an "Add to mission" mode.
- Lua export in two forms: readable (API calls) and raw (every field).
- City 3D view with the mission's actors, objects and cutscene cameras; camera shot timeline with preview; minimap.
- Vulkan renderer with a CPU fallback; light and dark themes.
- Parallel Lines: models and textures, the city (terrain tiles and props), a minimap. Interiors are placed but not verified; the road data is not decoded.
- Supported: Windows 10 and Windows 11, 64-bit (x86_64) only.
