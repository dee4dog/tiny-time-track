# Tiny Time Track — design brief for claude.ai/design

Paste the **Style preamble** first (or at the top of any screen prompt), then add a
screen prompt. These are derived from the real app's CSS and templates so mockups
match production — a warm, modern, terracotta-and-pastel theme with a
stencil-grunge header.

---

## Style preamble (paste this first, every time)

> Design in this exact visual style — a warm, modern, terracotta-led B2B web app.
> System font stack (system-ui, Segoe UI, Roboto, Arial) for body text, ~1.55
> line-height.
>
> Colors:
> - Accent / primary: terracotta #c2674a (hover #a8543a)
> - Soft accent washes: #f5e2d8, and a faint tint #fbeee7 (used for table heads,
>   row hovers, chips)
> - Page background: warm cream #faf2ec, with a soft radial terracotta glow in the
>   top-right corner
> - Card / surface: #fffaf6 (near-white, slightly warm)
> - Text: warm dark brown #3b2c25, muted text: taupe #997f72
> - Borders: soft sand #ecdbd0
> - Success/green: dusty sage #6f9d80 (soft bg #e3efe6)
> - Warning/amber: pastel ochre #cf9a4e (soft bg #f6ebd6)
> - Danger/red: muted brick #c0563e (soft bg #f7e0d8)
>
> Shape & spacing: cards and tables have a 14px radius, 1px #ecdbd0 borders, and
> soft warm shadows (e.g. 0 8px 24px rgba(120,72,52,0.07)); inputs/buttons 10px
> radius. Max content width 1080px, centered. Primary buttons are solid terracotta
> with white text and a soft drop shadow; secondary buttons are white with a
> border and a faint terracotta tint on hover. Inputs show a 3px soft terracotta
> focus ring. Keep it warm and calm with subtle depth — tiles lift slightly on
> hover. Tabular, right-aligned numeric columns in data tables; table headers are
> small, uppercase, letter-spaced and muted on a faint terracotta tint.
>
> Header / branding: a sticky, frosted (blurred translucent) top bar. The brand
> "Tiny Time Track" sits left in a **stencil font with a grunge/distressed texture**,
> uppercase, terracotta. Nav links are pill-shaped and tint terracotta on hover;
> user name + "Log out" on the right. Page titles (h1) also use the uppercase
> stencil-grunge treatment; sub-headings and body stay in the clean system font.

---

## Screens

### 1. Sign in
> A centered login card (max 460px, soft warm shadow, ~8vh from top) on the cream
> background with its top-right terracotta glow. Title "Tiny Time Track" in the
> uppercase stencil-grunge style, terracotta. Muted subtitle "Sign in to your
> timesheet." Stacked Email and password fields with small muted labels above
> each (terracotta focus ring), then a full-width solid terracotta "Sign in"
> button. No top bar on this screen.

### 2. Home / role landing
> A "Welcome, {name}" stencil-grunge heading, muted "You are signed in as {role}."
> line, then a responsive grid of clickable tiles (warm white cards, soft shadow,
> terracotta-coloured tile headings, muted one-line descriptions) that lift on
> hover: "My timesheet — Plan your week and log actual hours.", "Manager dashboard
> — Project profitability, people, and weekly compliance.", "Settings — Employees,
> projects, reminders and globals."

### 3. Employee timesheet (the core screen)
> An Excel-style weekly timesheet grid. Header row: a stencil-grunge "My timesheet"
> title; week navigation (prev/next + date picker, "Week of …" label) on the left,
> and a **pill-shaped Plan/Actual segmented toggle** on the right (active segment
> solid terracotta, rounded). Status chips show submission state (neutral "not
> submitted" / sage-green "submitted").
>
> The grid: rows are projects (left column, project name + a small terracotta-tint
> tag for non-billable), columns are Mon–Fri (each header shows the weekday and the
> date underneath), plus a Week total column. Each cell is a **single** small
> centered number input (hours) with a soft terracotta focus ring. A per-row total
> column and a grand-total footer row.
>
> Day totals are colour-coded: sage-green when exactly 8h, ochre-amber when under
> 8h, brick-red when over 8h (anything over 8h/day counts as overtime). Below the
> grid: "Submit plan", a solid-terracotta "Submit actuals", and "Copy last week's
> plan", with an "Add project" dropdown aligned right. Compact and
> keyboard-navigable in feel.

### 4. Manager dashboard — Projects
> A manager dashboard titled in stencil-grunge, with three pill/underline tabs
> (Projects / People / This week), active tab in terracotta. Projects tab: a muted
> intro line (cost-to-date uses each person's salary in effect that week; hours
> over 8/day are costed at an overtime factor), an "Export to Excel" secondary
> button, then a sortable data table with uppercase letter-spaced headers:
> Project, Fee, Hours, Cost to date, Profit, Margin, and "Last 4 wks" (a small
> terracotta burn sparkline). Money right-aligned with tabular numbers; negative
> profit in brick-red. Non-billable projects show a small terracotta-tint
> "non-billable" pill. Rows are clickable to open project detail; a totals footer
> row sits on a faint terracotta tint.

### 5. Manager dashboard — Project detail
> A project detail page: the project name as a heading, a row of small stat cards
> (Fee, Cost to date, Profit, Margin — big bold values, small uppercase muted
> labels, negative in brick-red, soft shadow), then two charts side by side in warm
> white chart cards: "Cost burn vs fee" (terracotta cumulative-cost line with a
> soft terracotta fill, dashed brick-red fee line) and "Planned vs actual hours"
> (terracotta "Planned" bars, ochre "Actual" bars), ~260px tall. Below, a "By
> employee" table (Employee, Hours, Overtime, Cost) and an audited fee editor: a
> labelled currency input and a solid-terracotta "Update fee" button.

### 6. Manager dashboard — People
> People tab: a data table with uppercase headers — Employee, Capacity, Planned,
> Actual, Overtime, Utilisation, and a 6-week compliance indicator rendered as a
> row of small rounded square dots (sage-green = submitted, warm light grey =
> missed). Overtime is the derived hours over 8/day; utilisation as a percentage.

### 7. Manager dashboard — This week
> A submission board: a table of people with their submission status as pills
> (sage-green "ok" / brick-red "miss"), and a per-row "Send reminder" small button.
> A row highlights ochre-amber when a reminder failed.

### 8. Settings
> A manager settings page with sections in warm white form cards (max 720px, soft
> shadow). Two-column grid forms with small muted labels: global settings (overhead
> multiplier, overtime factor, available-hours default, branding), SMTP config with
> a "Send test email" button, and a Reminders section with editable email templates
> (subject + textarea, {name}/{week}/{link} placeholders) and an editable schedule.
> Calm, warm, dense, form-heavy.
