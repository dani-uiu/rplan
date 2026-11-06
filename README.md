# rplan

A fast, plain-text alternative to ProjectLibre, written in Rust. It's one small program with no dependencies. It gives you a desktop app, a command line, and project files you can read, diff and keep in git. The interface is in English and Spanish.

```
rplan app "My Project.plan"     # desktop window
rplan ui  "My Project.plan"     # same editor in a browser tab
```

## Features

### Scheduling
- **Critical path over real working time**, down to the minute.
- **Summary tasks** (outline levels), with links on summaries.
- **All four link types** (FS, SS, FF, SF), with a lag, a lead, or calendar-time lag (`+12eh`).
- **All eight constraint types:** as soon / as late as possible, start no earlier / no later than, finish no earlier / no later than, must start on, must finish on.
- **Deadlines, milestones and free slack.**
- **Effort-driven tasks** (`work=40h`): adding people shortens the task. Overtime hours are supported.
- **Durations in calendar time** (`3ed`): nights and weekends count, for things like concrete curing.
- **Paused tasks** that skip given days.
- **Manually scheduled and inactive tasks.**
- **Leveling priority** from 0 to 1000.

### Calendars
- **Per-weekday working hours** and a configurable length for one "day" of duration.
- **Holidays and extra working days.**
- **Holidays that repeat every year:**
  - a fixed date (`12-25`)
  - the nth weekday of a month (`3rd Mon Jan`, `last Mon May`)
  - days relative to Easter (`easter-2`)
  - rules that move a holiday off the weekend (`observed`, `monday`, `substitute`)
- **One-click national holidays** for Colombia, Spain, Mexico, the UK and the US.
- **Each person's own working week and days off.** A task with several people runs only when all of them are working.
- **Named calendars** for individual tasks, such as a night shift.

### People and costs
- **Three resource types:** people (hourly rate, overtime rate, cost per use, part-time capacity), materials (cost per unit, e.g. `Cement[20]`) and one-off costs (`Travel[$400]`).
- **Leveling delays the most flexible, lowest-priority clashing tasks.** It picks whatever moves the finish date least, adds no links, and can be undone.

### Tracking
- **Actual start, finish and cost** for each task.
- **A status date.** Unstarted and unfinished work is moved after it.
- **Up to 11 baselines,** each with cost and work. Slip is shown per task and for the whole project.
- **Earned value:** BAC, PV, EV, AC, SV, CV, SPI, CPI and the estimate at completion.

### Several projects
- **A shared team file** (`pool team.plan`). Workloads add up across every project that uses it.
- **Sub-projects:** a task can stand for another `.plan` file (`sub=Other.plan`).
- **Links to tasks in other projects** (`ext=Other.plan#5FS+1d`).

### The app
- **Views:** Gantt, Network, WBS chart, Usage (hours by task or by person per day, week or month, plus a workload chart) and People.
- **Gantt time scales:** day, week, month, quarter and year. Only the rows on screen are drawn, so large plans stay fast. With 2,000 tasks, a redraw takes about 30 ms.
- **Search, filters, sorting and grouping** (by person, phase, status or criticality), and a choice of which columns to show.
- **Details panel:** links and lag, people and amounts, actual progress, constraints, calendar, pauses, sub-project, earned value and baselines.
- **Live warnings while you edit**, such as "this pushes the finish by 2 days", "Ana is now over-booked" or "this task will miss its deadline". The 🔔 button lists every current problem.
- **Printable reports:** project summary, who does what, cash flow, earned value (with a chart), task list, and problems & late tasks. You choose the paper size and orientation, pages are numbered, and "Save as PDF" in the print dialog makes a file to share.
- **Version history:** every change is kept in `.rplan-history/`. You can see what restoring an old version would change, then restore it.
- **Editing:**
  - drag bars to move them, drag their right edge to resize, drag their ○ handle onto another bar to link them
  - drag rows to reorder them
  - multi-select, then copy or paste rows (also to and from spreadsheets)
  - recurring tasks
  - undo and redo
- **Open or create projects from inside the app** (<kbd>Ctrl</kbd>+<kbd>O</kbd>).
- **Microsoft Project XML import and export** (MSPDI, which ProjectLibre reads and writes). This covers actual dates, effort, resource types, all constraint types, baselines and the status date.

## The `.plan` format

```text
project Website Launch
start 2026-11-09
status 2026-11-18
pool team.plan
week Mon-Fri 08:00-12:00 13:00-17:00
week Sat,Sun off
holiday 2026-11-26 Thanksgiving
holiday every 12-25 observed Christmas
holiday every easter-2 Good Friday
workday 2026-12-05 09:00-13:00 Release Saturday
calendar Night | week Mon-Fri 22:00-24:00 | week Sat,Sun off
resource Ana | rate 85 | overtime 120 | peruse 20 | capacity 100 | week Mon-Wed 08:00-17:00 | week Thu-Sun off | off 2026-11-20
resource Cement | type material | unit bags | rate 9.5
resource Travel | type cost
# id | name | duration | predecessors | resources | % done | attributes
task 10 | Design phase |
  task 2 | Requirements | 3d | | Ana | 100 | as=2026-11-09T08:00; af=2026-11-11T17:00
  task 3 | UX design | 5d | 2FS+1d | Ana[50%], Cement[20], Travel[$400] | 60 | work=40h; snet=2026-11-12; deadline=2026-11-20
task 4 | Cure | 3ed | 3 | | | ext=Supplier.plan#7; priority=800
task 5 | Launch | 0 | 10,4 | | | base=2026-12-08T17:00/2026-12-08T17:00/0/0
```

- **Outline:** indent by two spaces per level to make the task above a summary task.
- **Durations:** `3d`, `4h`, `30m`, `2w`, `3ed` (calendar days). `0` is a milestone.
- **Links:** `3`, `3SS`, `3FF+2d`, `3-4h`.
- **Task attributes:**

  | Attribute | Meaning |
  |---|---|
  | `snet`, `snlt`, `fnet`, `fnlt`, `mso`, `mfo` | constraint with a date |
  | `alap` | as late as possible |
  | `deadline` | finish-by date |
  | `work` | effort (makes the task effort-driven) |
  | `cost` | fixed cost |
  | `as`, `af`, `ac` | actual start, actual finish, actual cost |
  | `ot` | overtime per person |
  | `split` | paused days |
  | `manual` | manual start |
  | `inactive` | keep the task but ignore it |
  | `priority` | leveling priority, 0–1000 |
  | `cal` | named calendar |
  | `sub` | sub-project file |
  | `ext` | link to another project's task |
  | `base`, `base1`…`base10` | baselines |
  | `delay` | leveling delay |
  | `note` | free text |

Older files still load.

## Command line

```
rplan app <file>                            desktop window
rplan ui <file> [--port 7878] [--no-open]   browser tab
rplan init <file> "<name>" <YYYY-MM-DD>     new project        rplan demo <file>   sample project
rplan import <in.xml> <file>                from ProjectLibre / MS Project XML
rplan export <file> <out.xml>               to ProjectLibre / MS Project XML
rplan html <file> <out.html>                shareable Gantt
rplan show | gantt | resources <file>       terminal reports
rplan add | set | rm | indent | outdent ...  edit tasks (see rplan --help)
rplan week | dayhours | holiday | workday | resource ...  calendar & people
rplan holidays <file> <CO|ES|MX|UK|US>      add a country's public holidays
rplan status <file> <DATE|today|none>       status date for progress & earned value
rplan level | unlevel <file>                resource leveling
rplan baseline <file> [0-10] [clear]        save / clear a baseline
```

## Security

The app listens only on `127.0.0.1`, and checks every request:

- The `Host` header must name this server, which blocks DNS-rebinding attacks.
- Requests from other websites (a foreign `Origin`) are refused.
- Every API call must carry a per-session token embedded in the page.

The page only runs rplan's own scripts (`script-src 'self'`).

Saves are protected too:

- A save is refused if the file changed since you loaded it.
- Writes are atomic.
- A plan that can't be scheduled is never saved.
- Opening and creating projects is limited to the current project's folder.

## Building and testing

```
cargo build --release
cargo install --path .
cargo test                                          # 38 tests: scheduler, calendars, file format, XML, history, linked files, server security
node tests/ui_test.mjs target/release/rplan.exe     # 38 tests driving the real UI in headless Edge/Chrome
```

GitHub Actions runs both suites on Windows, macOS and Linux.

## Code layout

| Path | What it holds |
|---|---|
| `src/calendar.rs` | working time, holidays, repeating dates, calendar intersection |
| `src/model.rs` | project model and the `.plan` format |
| `src/schedule.rs` | critical path, actuals, earned value, costs, workloads, leveling, baselines |
| `src/external.rs` | shared team files, sub-projects, links to other projects |
| `src/history.rs` | version history |
| `src/mspdi.rs` | Microsoft Project XML |
| `src/report.rs` | terminal reports and HTML export |
| `src/server.rs` | local server, security, app window |
| `src/ui/` | the interface (built into the program) |
| `tests/` | browser-driven tests and fixtures |
