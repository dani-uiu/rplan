# rplan

A fast, plain-text alternative to ProjectLibre, written in Rust. It's a single program with no dependencies. You get a visual editor in your browser, a command line, and project files you can read, diff and keep in git.

```
rplan ui "My Project.plan"
```

## What it does

- **Scheduling:** critical path over real working time (minutes), with:
  - summary tasks (outline levels)
  - all four link types (FS, SS, FF, SF), with lag or lead
  - constraints: start no earlier than, finish no earlier than, must start on
  - deadlines and milestones
- **Calendars:**
  - per-weekday working hours, such as `Mon-Thu 08:00-12:00 13:00-17:00` and `Fri 08:00-12:00`
  - holidays and date ranges
  - extra working days with their own hours
  - hours per "day" of duration
  - personal days off for each person
- **People and costs:** hourly rates, part-time capacity (e.g. 50%), assignment units (`Bo[50%]`), fixed costs, and cost totals rolled up through summary tasks.
- **Resource leveling:** clashing tasks are delayed until nobody is over-booked. For each clash it tries every option and keeps the one that moves the finish date least. It adds no links, and you can undo it.
- **Tracking:**
  - % complete
  - baselines, with slip per task and for the whole project
  - "past deadline", "behind schedule" and "constraint conflict" warnings
- **Interoperability:**
  - imports and exports Microsoft Project XML (MSPDI), the format ProjectLibre, MS Project and GanttProject read and write
  - exports a standalone HTML Gantt chart
  - prints or saves to PDF

### The visual editor (`rplan ui`)

- **Gantt:**
  - editable table with collapsible summaries and WBS numbers
  - drag a bar to move it, drag its right edge to change the duration, drag its ○ handle onto another bar to link them
  - drag ⋮⋮ to reorder rows
- **Details panel:** opens on double-click or <kbd>Enter</kbd>. Edit link types and lags, people and units, constraints, deadlines, fixed cost and notes.
- **Calendar editor:** working week, presets (8–5, 9–6, Mon–Sat, 4×10, 24/7), holidays and exceptions, and a month preview you can click.
- **Network:** a task-card diagram with the critical path highlighted.
- **People:**
  - a workload heat map with over-capacity days in red
  - editable rates, capacity and days off
- **Editing shortcuts:**
  - multi-select (<kbd>Shift</kbd>/<kbd>Ctrl</kbd>+click)
  - copy rows, or paste them from a spreadsheet
  - <kbd>Tab</kbd> to indent
  - undo and redo
  - light and dark themes
- **Live reload:** if you edit the `.plan` file in a text editor while the app is open, the app reloads it.

Press <kbd>?</kbd> in the app for all shortcuts.

## The `.plan` format

```text
project Website Launch
start 2026-11-09
currency $
week Mon-Fri 08:00-12:00 13:00-17:00
week Sat,Sun off
dayhours 8
holiday 2026-11-26 Thanksgiving
holiday 2026-12-24..2026-12-31 Winter break
workday 2026-12-05 09:00-13:00 Release Saturday
resource Ana | rate 85 | capacity 100 | off 2026-11-20..2026-11-21
resource Bo | rate 70 | capacity 50
# id | name | duration | predecessors | resources | % done | attributes
task 10 | Design phase |
  task 2 | Requirements | 3d | | Ana | 100
  task 3 | UX design | 5d | 2FS+1d | Bo[50%] | 60 | snet=2026-11-12; deadline=2026-11-20; cost=500; note=Talk to marketing
task 4 | Launch | 0 | 10 | |
```

- Indentation (two spaces per level) turns the task above into a summary task.
- Durations: `3d`, `1.5d`, `4h`, `30m`, `2w`. `0` is a milestone.
- Predecessors: `3` (finish-to-start), `3SS`, `3FF+2d`, `3SF`, `3-4h` (a lead).
- Task attributes:

  | Attribute | Meaning |
  |---|---|
  | `snet` | start no earlier than |
  | `fnet` | finish no earlier than |
  | `mso` | must start on |
  | `deadline` | finish-by date |
  | `cost` | fixed cost |
  | `delay` | leveling delay |
  | `base` | baseline start/finish |
  | `note` | free text |

Older files (`task 1 | Name | 3 | 2 | Ana | 50`) still load.

## Command line

```
rplan ui <file> [--port 7878] [--no-open]   visual editor
rplan init <file> "<name>" <YYYY-MM-DD>     new project
rplan demo <file>                           sample project
rplan import <in.xml> <file>                from ProjectLibre / MS Project XML
rplan export <file> <out.xml>               to ProjectLibre / MS Project XML
rplan html <file> <out.html>                shareable Gantt
rplan show | gantt | resources <file>       reports in the terminal
rplan add <file> "<name>" <dur> [--after 2,3SS] [--who "Ana,Bo[50%]"] [--under <id>]
rplan set <file> <id> [--name] [--dur] [--after] [--who] [--done] [--snet DATE|none] [--deadline DATE|none] [--cost] [--note]
rplan rm | indent | outdent <file> <id>
rplan week <file> <Mon-Fri> <08:00-12:00 13:00-17:00 | off>
rplan dayhours <file> <hours>
rplan holiday <file> <DATE[..DATE]> [name]
rplan workday <file> <DATE[..DATE]> [HH:MM-HH:MM ...] [name]
rplan resource <file> <name> [--rate 85] [--capacity 50] [--off DATE[..DATE],...]
rplan level | unlevel <file>
rplan baseline <file> [clear]
```

## Security

`rplan ui` listens only on `127.0.0.1`, and checks every request:

- The `Host` header must name this server. This blocks DNS-rebinding attacks.
- Requests from another website (a foreign `Origin`) are refused.
- Every API call must carry a secret token that is generated each session and embedded in the page.

Other websites can't read the page, so they can't learn the token. A custom header can't be sent cross-site without a CORS preflight, which the server never approves.

Saves are also protected:

- A save is refused if the file changed on disk since you loaded it. The app reloads instead of overwriting.
- Files are written atomically.
- A plan that can't be scheduled is never saved.

## Building and testing

```
cargo build --release            # target/release/rplan(.exe)
cargo install --path .           # puts rplan on your PATH
cargo test                       # scheduler, calendar, file format, XML, server security
node tests/ui_test.mjs target/release/rplan.exe   # drives the real UI in headless Edge/Chrome
```

The UI tests need Node 22 or later and Chrome or Edge. Set `BROWSER=` to point at a different browser.

## Code layout

| File | What it holds |
|---|---|
| `src/calendar.rs` | dates, working hours, holidays, working-time math |
| `src/model.rs` | project model and the `.plan` format |
| `src/schedule.rs` | critical path, summaries, constraints, costs, workloads, leveling, baselines |
| `src/mspdi.rs` | Microsoft Project XML import/export (small built-in XML reader) |
| `src/report.rs` | terminal reports and HTML export |
| `src/server.rs` | local web server and its security checks |
| `src/ui.html` | the visual editor, built into the program |
| `tests/ui_test.mjs` | browser-driven end-to-end tests |
