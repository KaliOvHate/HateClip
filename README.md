> [!WARNING]
> **Disclaimer: this entire project is 100% clanker slop.**
> Every line of code, every config file, the icons and this README were written by an AI model (Claude).
> No human has reviewed, audited or tested the code beyond running it. Use it at your own risk, do not
> trust it with anything sensitive, and do not treat it as an example of good practice. See [DISCLAIMER.md](DISCLAIMER.md).

# HateClip

Clipboard history for macOS, written in Rust. HateClip lives in the menu bar,
remembers what you copy (text, formatted text, images and files), and pastes any
earlier item back with one shortcut.

## Using it

- Press **Cmd+Shift+V** anywhere to open the history panel.
- Type to search. **Tab** cycles the filters (All, Text, Images, Files, Pinned).
- **Enter** pastes into the app you were using. **Shift+Enter** pastes plain text.
  **Option+Enter** only copies. **Cmd+1** to **Cmd+9** paste the first nine items.
- **Cmd+P** pins an item so it is never removed. **Cmd+Delete** removes it.
- **Esc** closes the panel. **Cmd+,** opens Settings.

Auto-paste needs the Accessibility permission (System Settings, Privacy and
Security, Accessibility). HateClip asks for it on first launch. Without it,
HateClip copies the item and you press Cmd+V yourself.

Copies from password managers that mark their content as private
(nspasteboard.org conventions) are never recorded, and you can exclude any app
in Settings.

## Building the .dmg

On a Mac with Xcode Command Line Tools and Rust (https://rustup.rs) installed:

```sh
scripts/build-dmg.sh            # universal: Apple Silicon and Intel
scripts/build-dmg.sh --native   # this Mac only, faster
```

The finished files are in `target/universal-apple-darwin/release/bundle/`
(or `target/release/bundle/` for `--native`): `dmg/HateClip_<version>_<arch>.dmg`
and `macos/HateClip.app`.

Without an Apple Developer ID the app is ad-hoc signed. It runs on the Mac that
built it. On another Mac, macOS blocks the first launch until you approve it under
System Settings, Privacy and Security, Open Anyway. Ad-hoc signed builds also get
a new identity on every rebuild, so macOS asks for the Accessibility permission
again after each new build. A Developer ID signature and notarization remove both
problems.

## Development

```sh
cargo test --workspace          # core tests run on any OS
cargo tauri dev --config app/tauri.conf.json
```

Layout:

| Path | Contents |
|---|---|
| `crates/hateclip-core` | Item model, settings, SQLite history store, thumbnails. Platform independent and unit tested. |
| `crates/hateclip-macos` | Pasteboard read and write, frontmost app, Accessibility check, Cmd+V simulation, panel window setup. |
| `app` | Tauri application: tray menu, global shortcut, windows, commands, clipboard watcher. |
| `ui` | Panel, Settings and Welcome pages (plain HTML, CSS and JavaScript, no build step). |

Data lives in `~/Library/Application Support/io.github.kaliovhate.HateClip/`, logs in
`~/Library/Logs/io.github.kaliovhate.HateClip/`.

## Uninstalling

Quit HateClip from the menu bar, drag it from Applications to the Trash, and
optionally delete the two folders above.

## Disclaimer

> [!WARNING]
> **Disclaimer: this entire project is 100% clanker slop.**
> Every line of code, every config file, the icons and this README were written by an AI model (Claude).
> No human has reviewed, audited or tested the code beyond running it. Use it at your own risk, do not
> trust it with anything sensitive, and do not treat it as an example of good practice. See [DISCLAIMER.md](DISCLAIMER.md).
