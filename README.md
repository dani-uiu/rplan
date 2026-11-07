# rplan

**rplan is a project scheduler written in Rust: a fast, plain-text alternative to ProjectLibre and Microsoft Project.**

- **A desktop app** with its own window, a Gantt chart, network and WBS diagrams, workload views, reports and a print studio.
- **A command line** for the same plans, for scripts and terminals.
- **Plain-text project files** (`.plan`) you can read, diff and keep in git.
- **It imports and exports Microsoft Project XML**, the format ProjectLibre also reads and writes.
- **One small program** that keeps everything local on your computer: no account, no cloud, no database.
- **The interface is in English and Spanish.**

```
rplan app "My Project.plan"     # open (or create) a project in the app window
```

## Contents

- [Installation](#installation)
- [Getting started](#getting-started)
- [What it does](#what-it-does)
- [The `.plan` format](#the-plan-format)
- [Command line](#command-line)
- [Security](#security)
- [Development](#development)

## Installation

### What you need

- **Rust** (stable, 1.82 or newer), from [rustup.rs](https://rustup.rs).
- **Windows 10 or 11:** nothing else. The app window uses the WebView2 runtime, which comes with Windows. If WebView2 is missing, rplan opens in an Edge or Chrome app window instead.
- **macOS:** the app window uses the system's WebKit. It builds, but it hasn't been tested on a Mac yet.
- **Linux:** the editor opens in Chrome, Chromium or Edge as an app window, or in your default browser.

### Build and install

```
git clone https://github.com/dani-uiu/rplan.git
cd rplan
cargo install --path .
```

This builds an optimized `rplan` and puts it in `~/.cargo/bin`, which rustup adds to your `PATH`. Check it with `rplan --help`.

To update later, run `git pull` and then `cargo install --path .` again.

### Optional (Windows): open `.plan` files with a double-click

Run this in PowerShell from the `rplan` folder. It only touches your own user account (`HKCU`):

```powershell
$exe = "$env:USERPROFILE\.cargo\bin\rplan.exe"
New-Item -ItemType Directory -Force "$env:LOCALAPPDATA\rplan" | Out-Null
Copy-Item assets\rplan.ico "$env:LOCALAPPDATA\rplan\rplan.ico"
$c = 'HKCU:\Software\Classes'
New-Item -Force "$c\.plan" -Value 'rplan.project' | Out-Null
New-Item -Force "$c\rplan.project" -Value 'rplan project' | Out-Null
New-Item -Force "$c\rplan.project\DefaultIcon" -Value "$env:LOCALAPPDATA\rplan\rplan.ico,0" | Out-Null
New-Item -Force "$c\rplan.project\shell\open\command" -Value "`"$exe`" app `"%1`"" | Out-Null
```

## Getting started

```
rplan demo sample.plan          # a sample project to explore
rplan app sample.plan           # open it in the app window
rplan app "Office Move.plan"    # a new, empty project (the file is created)
rplan ui sample.plan            # the same editor in a browser tab
```

In the app:

- Type a task name in the **Add a task…** row and press <kbd>Enter</kbd>.
- Durations go in the **Dur** column (`3d`, `4h`, `2w`). Predecessors go in **After** (`2`, `3SS+1d`), and people in **Who** (`Ana, Bo[50%]`).
- Drag bars to move them. Drag their right edge to change the duration, and drag the ○ handle onto another bar to link the two.
- <kbd>Enter</kbd> opens the details panel. <kbd>?</kbd> lists every keyboard shortcut.
- Every change is saved straight to the `.plan` file. <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes it, and **Project ▸ Version history** goes further back.

## What it does

### Scheduling
- **Critical path over real working time**, down to the minute, with free and total slack.
- **Summary tasks** (outline levels), milestones and deadlines.
- **All four link types** (FS, SS, FF, SF). A link can have a lag, a lead, a calendar-time lag (`+12eh`) or a share of the predecessor (`+50%`).
- **All eight constraint types:**
  - as soon as possible, as late as possible
  - start no earlier than, start no later than
  - finish no earlier than, finish no later than
  - must start on, must finish on
- **Three task types:**
  - fixed duration (the default)
  - fixed units: changing the work changes the duration
  - fixed work (effort-driven): adding people shortens the task
- **Durations** in minutes, hours, days, weeks or months (`2mo` = 40 working days), or in calendar time (`3ed`), where nights and weekends count.
- **More kinds of tasks:**
  - paused tasks that skip given days
  - manually scheduled tasks
  - inactive tasks, kept but ignored
  - recurring tasks
  - sub-projects (a task that stands for another `.plan` file)
  - links to tasks in other projects

### Calendars
- **Working hours for each weekday**, and how many hours one "day" of duration means.
- **Holidays and extra working days.**
- **Holidays that repeat every year:**
  - fixed dates (`12-25`)
  - the nth weekday of a month (`3rd Mon Jan`)
  - dates relative to Easter (`easter-2`)
  - rules that move a holiday off the weekend (`observed`)
- **One-click national holidays** for Colombia, Spain, Mexico, the UK and the US.
- **Each person's own working week and days off.**
- **Named calendars** for individual tasks, such as a night shift.

### People and costs
- **People, materials and one-off costs:**
  - people have an hourly rate, an overtime rate, a cost per use and part-time capacity
  - materials have a cost per unit (`Cement[20]`)
  - one-off costs are a fixed amount (`Travel[$400]`)
- **Each person's own part of a task:**
  - a later start (`delay=1d`)
  - their own hours (`work=16h`)
  - a work contour: flat, front-loaded, back-loaded, bell, early peak, late peak or turtle
- **Rates and capacity that change by date.** Each person's cost can count as the work is done, at the start, or at the end.
- **People details:** group, initials, email and notes. A resource breakdown (RBS) chart is built from the groups.
- **A shared team file** for several projects, so workloads add up across all of them.
- **Resource leveling** delays the most flexible, lowest-priority clashing work. It never adds links, and it can be undone. Options:
  - stay within slack (never move the finish date)
  - pause work that's under way instead of delaying it
  - choose which task moves first

### Tracking
- **Actual start, finish and cost**, and a status date. Unfinished work moves after the status date.
- **"Update progress to the status date"** in one click.
- **Up to 11 baselines**, with the slip shown per task and for the whole project. The **tracking Gantt** shows each bar above its baseline.
- **Earned value:** BAC, PV, EV, AC, SV, CV, SPI and CPI.

### Custom fields
- **Text, number, date, yes/no and outline codes** with levels (`Eng.Backend`). Use them as Gantt columns, group by them, and print them.

### The app
- **Views:**
  - Gantt, with day, week, month, quarter and year scales
  - network diagram
  - WBS chart
  - usage by task or person, and a workload chart
  - people, as a table or as the RBS chart
- **Search, filters, sorting and grouping**, and a choice of columns.
- **Live warnings while you edit**, such as "this pushes the finish by 2 days" or "Ana is now over-booked".
- **Chart styles:** Modern, Corporate, Classic, Executive and Black & white, each in dark or light mode.
- **App zoom** from 50% to 250% (<kbd>Ctrl</kbd> <kbd>+</kbd> / <kbd>−</kbd> / <kbd>0</kbd>).
- **Print studio** (<kbd>Ctrl</kbd>+<kbd>P</kbd>) with a page-by-page preview:
  - paper, margins, and fit to N pages or a scale
  - headers and footers with page numbers
  - rows, columns and date range
  - a legend and a print-only style
  - Gantt pages are drawn as vector graphics; use "Save as PDF" to share them.
- **Reports:** project summary, who does what, cash flow, earned value, task list, and problems.
- **Version history** of every change, with a preview of what restoring would change.
- **Microsoft Project XML import and export**, plus a shareable HTML Gantt.
- **Fast with large plans:** only the rows on screen are drawn. With 2,000 tasks, a redraw takes about 30–50 ms.

## The `.plan` format

One line per setting, person or task:

```text
project Website Launch
start 2026-11-09
status 2026-11-18
week Mon-Fri 08:00-12:00 13:00-17:00
week Sat,Sun off
holiday 2026-11-26 Thanksgiving
holiday every 12-25 observed Christmas
calendar Night | week Mon-Fri 22:00-24:00 | week Sat,Sun off
resource Ana | rate 85 | rate 95 from 2027-01-01 | capacity 50 from 2027-03-01 | group Eng/Backend | off 2026-11-20
resource Cement | type material | unit bags | rate 9.5
field Dept | code | Eng, Eng.Backend, Sales
# id | name | duration | predecessors | resources | % done | attributes
task 10 | Design phase |
  task 2 | Requirements | 3d | | Ana | 100 | as=2026-11-09T08:00; af=2026-11-11T17:00
  task 3 | UX design | 5d | 2FS+1d | Ana[50%], Cement[20] | 60 | work=40h; deadline=2026-11-20
task 4 | Review | 2d | 3FS+50% | Bo[50% delay=4h contour=bell] | | x:Dept=Eng.Backend
task 5 | Launch | 0 | 10,4 | | | base=2026-12-08T17:00/2026-12-08T17:00/0/0
```

- **Outline:** indent two spaces per level. The task above becomes a summary task.
- **Durations:** `3d`, `4h`, `30m`, `2w`, `2mo`, `3ed`. `0` is a milestone.
- **Links:** `3`, `3SS`, `3FF+2d`, `3-4h`, `3FS+50%`.
- **People:** `Ana`, `Bo[50%]`, `Cement[20]`, `Travel[$400]`, and details in the brackets, like `Ana[50% delay=1d work=16h contour=back]`.
- **Project lines:**

  | Line | Meaning |
  |---|---|
  | `project`, `start`, `status`, `currency` | basics |
  | `week`, `dayhours` | working week, hours in a day |
  | `holiday`, `workday`, `holiday every …` | exceptions and repeating holidays |
  | `calendar` | named calendars |
  | `pool` | shared team file |
  | `resource` | people, materials and costs. Fields: `type`, `rate` (also `rate X from DATE`), `capacity` (also `capacity X from DATE`), `peruse`, `overtime`, `unit`, `accrue`, `group`, `initials`, `email`, `note`, `week`, `off` |
  | `field` | custom fields and outline codes |
  | `leveling` | leveling options: `slack`, `split`, `order=standard`, `order=priority` or `order=id` |

- **Task attributes:**

  | Attribute | Meaning |
  |---|---|
  | `snet`, `snlt`, `fnet`, `fnlt`, `mso`, `mfo`, `alap` | constraints |
  | `deadline` | finish-by date |
  | `work`, `type=units` | effort, and the fixed-units task type |
  | `cost` | fixed cost |
  | `as`, `af`, `ac` | actual start, actual finish, actual cost |
  | `ot` | overtime per person |
  | `split` | paused days |
  | `lsplit` | pauses added by leveling |
  | `delay` | leveling delay |
  | `manual` | manual start |
  | `inactive` | keep the task but ignore it |
  | `priority` | leveling priority, 0–1000 |
  | `cal` | named calendar |
  | `sub`, `ext` | sub-project file, link to another project's task |
  | `base`, `base1`…`base10` | baselines |
  | `x:Name` | a custom field's value |
  | `note` | free text |

Files from older versions still load.

## Command line

```
rplan app <file>                            app window
rplan ui <file> [--port 7878] [--no-open]   browser tab
rplan init <file> "<name>" <YYYY-MM-DD>     new project
rplan demo <file>                           sample project
rplan import <in.xml> <file>                from ProjectLibre / MS Project XML
rplan export <file> <out.xml>               to ProjectLibre / MS Project XML
rplan html <file> <out.html>                shareable Gantt
rplan show | gantt | resources <file>       terminal reports
rplan add | set | rm | indent | outdent …   edit tasks (see rplan --help)
rplan week | dayhours | holiday | workday | resource …   calendar & people
rplan holidays <file> <CO|ES|MX|UK|US>      a country's public holidays
rplan status <file> <DATE|today|none>       status date
rplan progress <file>                       progress as planned up to the status date
rplan level | unlevel <file>                resource leveling
rplan baseline <file> [0-10] [clear]        save / clear a baseline
```

## Security

The editor is served only on `127.0.0.1`, and every request is checked:

- The `Host` header must name this server, which blocks DNS rebinding.
- Requests from other websites (a foreign `Origin`) are refused.
- Every API call needs a per-session token.
- The page only runs rplan's own scripts (`script-src 'self'`).

Saves are protected too:

- A save is refused if the file changed since it was loaded.
- Writes are atomic.
- A plan that can't be scheduled is never saved.
- Opening and creating projects is limited to the project's own folder.

## Development

```
cargo build --release
cargo test --release                                # scheduler, calendars, file format, XML, history, linked files, server security
node tests/ui_test.mjs target/release/rplan.exe     # drives the real interface in headless Edge/Chrome (Node 22+)
```

GitHub Actions runs both test suites on Windows and Linux for every push.

| Path | What it holds |
|---|---|
| `src/main.rs` | command line |
| `src/model.rs` | project model and the `.plan` format |
| `src/calendar.rs` | working time, holidays, repeating dates |
| `src/schedule.rs` | critical path, actuals, earned value, costs, workloads, leveling, baselines |
| `src/external.rs` | shared team files, sub-projects, links between projects |
| `src/history.rs` | version history |
| `src/mspdi.rs` | Microsoft Project XML |
| `src/report.rs` | terminal reports and HTML export |
| `src/server.rs` | local server, security, browser fallback |
| `src/window.rs` | the native app window |
| `src/ui/` | the interface, built into the program; `print.js` is the print studio |
| `vendor/webview2-com-sys` | WebView2 bindings, changed to load Microsoft's `WebView2Loader.dll` (in `assets/`) only when the window opens |
| `tests/` | browser tests and fixtures |
