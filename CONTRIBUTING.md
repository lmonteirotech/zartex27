# Contributing to Zartex'27

Thank you for wanting to help. Fixes, new features, documentation, translations, tests and bug reports are all welcome.

## The one rule: you must understand what you send

Any contribution is accepted, whatever tools were used to write it, **as long as the person who sends it knows exactly what each of their changes does**. Before you open a pull request you must:

- understand how the part of the code you changed works and where it sits in the program (see [`docs/architecture`](docs/architecture/) for the structure of the code);
- be able to explain every line of your change, and why it is needed;
- have run the program (or the tests) and seen the result with your own eyes.

If you cannot explain a change, do not send it.

## Tools and AI assistants

- Using editors, code generators, scripts and AI assistants to **write** a change is fine.
- Using AI tools as an extra **checker** is encouraged: security review, functionality, integrity, compatibility of the code, and finding mistakes before a maintainer does.
- An AI tool must **not** be the one that decides whether the result is correct. The review of what the code does, and of the result it produces, must be made by a **human**: you first, then a maintainer. A pull request that was only checked by a machine will not be merged.

Please say in the pull request description if a tool wrote a large part of the change. This is not a penalty; it only helps the review.

## Where the code comes from

Part of the core (`zartex-core`) is a conversion of the earlier tools Zartex and Zartex 2, made with the help of a language model and reviewed by a person. Every file says so in its header, and says what was adjusted, added or fixed in this repository. If you change one of those files, keep the header true: add a line about your change.

## Building and testing

Windows 10 or Windows 11, 64-bit, and the stable Rust toolchain are the only supported setup for now.

```
cargo build --release
cargo test --release
```

Some tests need a game installation. They are skipped unless you point them at one:

- `DRIV3R_DIR`: the folder of the first game (for the export and round-trip tests);
- `PL_DIR`: the folder of Driver: Parallel Lines (for the city-file tests).

The tests that do not need a game (the sample scripts, the golden outputs, the manual checks) always run.

## Game files and trademarks

Never add files from the games to this repository: no models, textures, sounds, missions or any other data taken from an installation, not even as test data. Tests must use files made for this project, or skip themselves when a game is not installed. Driv3r (Driver 3) and Driver: Parallel Lines, their names, logos, graphics, sounds and game files, are trademarks and copyrighted works of their respective owners. They are named in this project only to say which games the tool works with. Zartex'27 contains no file from the games, and it is not affiliated with, endorsed by or sponsored by their owners.

## Making a change

1. Open an issue first for anything big, so that we agree on the direction.
2. Make a branch, keep the change small and focused, and follow the style of the surrounding code (`cargo fmt` is not enforced; keep lines readable).
3. Add or update a test when you fix a bug or add a feature.
4. If you change a feature, a menu, a command-line argument or a Lua function, update the manual source in [`docs/manual-source`](docs/manual-source/). A test checks that the Lua functions and constants of the manual match the program.
5. Open the pull request, describe what changed and how you checked it.

## The manual

The manual in the releases is a PDF. Its editable source (text and images) is in `docs/manual-source/`. Edit `manual.md` and the images there; do not edit the PDF.

## The code-structure document

`docs/architecture/architecture.md` explains every source file (what it does, what it calls, what calls it). If you add, remove or rename a source file, or change what it calls, update that file. To rebuild the PDF: `python tools/md_to_docx.py docs/architecture/architecture.md out.docx "Zartex'27 Code Structure" "<footer text>"`, then update the fields and export to PDF from Word (needs `python-docx`).

## License

By contributing you agree that your work is released under the GNU General Public License, version 3 or (at your option) any later version, like the rest of the project. The software and the code are provided without any warranty.
