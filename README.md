# Riptag

Rip CDs straight into files your **FiiO Snowsky Echo Mini** handles properly: square JPEG cover art, clean tags, synced lyrics and a folder layout that browses well on the device. Also fixes music you already have.

Work in progress. macOS first, Windows later.

## Development

Requires Node 24 and Rust stable, plus the [Tauri prerequisites](https://tauri.app/start/prerequisites/).

```sh
npm install
npm run tauri dev
```

Converting audio needs `ffmpeg` installed. `npm run bundle` builds its own copy into the packaged app.

## License

Riptag is under the [MIT License](LICENSE). The packaged app also includes FFmpeg, under the LGPL 2.1 or later: see [its notice](licenses/ffmpeg-NOTICE.txt).
