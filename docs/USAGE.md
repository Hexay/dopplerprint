# Usage

## Browser app

```bash
dopplerprint serve --open
```

This opens a local page at `http://127.0.0.1:8737/`. Drop a recording on it, say when it started (a rough
time is fine; widen "give or take" to match) and, ideally, roughly where. Then press **Locate**. The page
shows progress and then the result: the best location and how sure it is, any mirror-image ambiguity, the
aircraft it heard, a map, and GeoJSON/JSON downloads. If something is missing, such as the start time, the
page says what it needs. Fill it in and press **Run again**; the recording doesn't have to be uploaded again.

| Option | Default | |
|---|---|---|
| `--port` | `8737` | If it is taken, the next free port up to +10 is used. `0` picks any free port. |
| `--host` | `127.0.0.1` | Only this computer can reach the page. `0.0.0.0` exposes it to your network with **no authentication**. |
| `--jobs` | `~/.cache/dopplerprint/jobs` | One folder per recording: `job.json` (form, status, progress, outcome), the audio, `results/`. |
| `--max-upload-mb` | `1024` | Larger uploads are refused. |
| `--open` | off | Opens the page in your default browser. |

Recordings are analysed one at a time, in the order they were submitted. Jobs survive a restart: queued
jobs resume, and a job that was running when the app stopped is marked as failed and can be run again.
Delete a job's folder under `--jobs` to forget it.

Uploads are checked by content (WAV, MP3, FLAC, OGG, M4A/AAC) and not by file name. They are stored
under a server-chosen name. The API (`/api/jobs`) is described at the top of `crates/dp-serve/src/http.rs`.
