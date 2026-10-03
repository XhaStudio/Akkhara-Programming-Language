# App — an Akkhara GUI library

Akhara's equivalent of Tkinter: windows, widgets, an event loop, dialogs and
a canvas. It is a **built-in** library compiled into the `akk` binary (like
`request` and `အချိန်`), because opening real windows can't be written in plain
Akkhara source. The GUI is drawn with [`egui`]/[`eframe`] (OpenGL), so an App
program needs no extra runtime and no Python.

[`egui`]: https://github.com/emilk/egui
[`eframe`]: https://github.com/emilk/egui/tree/master/crates/eframe

## Import

```
နည်းပညာများ App ကို အသုံးပြုပါ။
```

or, in the eng spelling:

```
use App;
```

## Design

- **Handles** — every `screen`/widget function returns a handle, which is an
  ordinary Akkhara object value (exactly like `request` returns a response
  object). Store it in a variable and pass it back to the other `App`
  functions. The object carries `id` (`ကိန်းပြည့်`) and `class`
  (`"window"` / `"widget"`), and widgets also carry `kind`
  (`"label"`, `"button"`, ...), so `b ၏ kind` works.
- **Callbacks by name** — a click handler is the *name* of an Akkhara
  function, given as text: `App.button(w, "Save", 20, 60, "on_save")`. When
  the user clicks, the interpreter calls that function (with no arguments).
  The function only has to exist by the time the button is clicked, so it can
  be defined below the window code.
- **Event loop** — build the whole UI first, then call `App.run(w)`. That
  shows the window and blocks until it closes (Tkinter's `mainloop()`).
  Widget changes made before `App.run` are already in place when the window
  appears.
- **Layout** — absolute `x`/`y` placement from the window's top-left corner,
  in pixels. Positive `x` is right, positive `y` is down.
- **One `App.run` at a time** — `App.run(w)` blocks; a window that is not
  currently run is shown but its buttons, keys and timers do not fire.

## Window

| Call | Meaning |
|---|---|
| `w = App.screen(400, 300)` | create a window `width` × `height` pixels, returns its handle. Nothing is shown until `App.run`. |
| `App.title(w, "My App")` | set the title-bar text (also works on a window that is already running) |
| `App.run(w)` | show the window and run the event loop until it closes |
| `App.close(w)` | close the window from code — e.g. from a "Quit" button's callback |

```
w အတွက် App ၏ screen(400, 300) ကို လုပ်ပါ။
App ၏ title(w, "My App") ကို လုပ်ပါ။
App ၏ run(w) ကို လုပ်ပါ။
```

## Widgets

Every widget call takes the window handle, its `x`, `y` position, and a
size where the widget needs one.

| Call | Meaning |
|---|---|
| `lb = App.label(w, "Name:", 20, 20)` | read-only text |
| `b = App.button(w, "Save", 20, 60, "on_save")` | clickable button; the last argument is the callback's function name (leave it out for a button that does nothing) |
| `box = App.input(w, 20, 100, 200)` | one-line text box, 200 pixels wide |
| `notes = App.textarea(w, 20, 140, 300, 100)` | multi-line text box, 300 × 100 |
| `c = App.checkbox(w, "Remember me", 20, 250)` | tick box with a caption |
| `d = App.choice(w, ["Red", "Green", "Blue"], 20, 280)` | dropdown built from a list |
| `li = App.listbox(w, ["Mon", "Tue"], 20, 20, 160, 100)` | scrolled list of rows; click a row to select it, scroll with the wheel |
| `tb = App.table(w, ["Name", "Age"], [["Aung", 30], ["Su", 25]], 200, 20, 220, 140)` | grid of cells; an empty `headers` list leaves the header row off; click a row to select it |
| `pic = App.image(w, "logo.png", 200, 20)` | picture; png, jpeg, bmp, gif and webp. The file's size becomes the widget's size unless `App.size` changes it |
| `cv = App.canvas(w, 0, 0, 400, 300)` | blank drawing area for `rect`/`circle`/`line`/`text` |

```
lb အတွက် App ၏ label(w, "Name:", 20, 20) ကို လုပ်ပါ။
b အတွက် App ၏ button(w, "Save", 20, 60, "on_save") ကို လုပ်ပါ။
box အတွက် App ၏ input(w, 20, 100, 200) ကို လုပ်ပါ။
```

## Reading and changing widget values

| Call | Meaning |
|---|---|
| `v = App.get(widget)` | current value: input/textarea text, a checkbox's `True`/`False`, the item a `choice`/`listbox` has selected (`""` when nothing is), or a label/button's caption / an image's path. A canvas has no single value (`E122`) |
| `App.set(widget, value)` | change it. Text widgets take anything (numbers and booleans are written as text), a checkbox wants `True`/`False`, and a `choice`/`listbox` wants one of its own items |
| `n = App.number(widget)` | the same value read as a number, so a text box can be used in arithmetic. Also takes a piece of text: `App.number("12.5")` |
| `App.get(li)` / `App.set(li, "Tue")` | a listbox's selected item, like a choice (`""` clears the selection) |
| `App.get(tb)` / `App.set(tb, 1)` | a table's selected row: `get` hands back its cells as a list (`[]` when nothing is selected), `set` takes a row number (`-1` clears the selection) |
| `list = App.items(w)` | every item of a `choice`/`listbox`, or every row of a `table` (each row a list of its cells) |
| `App.set_items(w, list)` | replace those items/rows; a flat list given to a table means one-cell rows |
| `App.cell(tb, 1, 0)` / `App.set_cell(tb, 1, 1, 26)` | read or change one table cell (numbers and booleans are written as text) |

```
App ၏ set(box, "Hello") ကို လုပ်ပါ။
name အတွက် App ၏ get(box) ကို လုပ်ပါ။
name ကို ဖော်ပြပါ။
```

`App.get` always hands back the text exactly as it stands in the box, so use
`App.number` whenever the value is a number:

```
App ၏ set(box, "12.5") ကို လုပ်ပါ။
x အတွက် App ၏ number(box) ကို လုပ်ပါ။
x ကို 1 တိုးပါ။
x ကို ဖော်ပြပါ။        # 13.5
```

`App.number` reads an integer as `ကိန်းပြည့်` and anything with a decimal
point as `ဒဿမ` (Myanmar digits count, so a box holding `၅` is five). An empty
box — or empty text — counts as `0`; anything else that isn't a number (say
`"twelve"`) raises `E128`, and a checkbox's `True`/`False` is `E126`: read it
with `App.get` instead.

### Lists and tables

`App.listbox` and `App.table` are the two widgets that hold rows of items.
Both scroll with the mouse wheel while the pointer is over them, and both
highlight the row the user clicks. A table draws its columns at equal widths.

```
li = App.listbox(w, ["Mon", "Tue", "Wed"], 20, 20, 160, 100)
App.set(li, "Tue")            # select an item
print(App.get(li))            # Tue
App.set_items(li, ["A", "B"])  # replace the items
print(App.items(li))          # ["A", "B"]

tb = App.table(w, ["Name", "Age"], [["Aung", 30], ["Su", 25]], 200, 20, 220, 140)
App.set(tb, 1)                # select row 1
row = App.get(tb)             # ["Su", "25"]
print(row[0])                 # Su
print(App.cell(tb, 1, 1))     # 25
App.set_cell(tb, 1, 1, 26)    # change one cell
```

A selection survives `App.set_items` only while the same item (or the same
row number) still exists; otherwise it is cleared. `App.set(li, "")` and
`App.set(tb, -1)` clear a selection directly. An empty `headers` list leaves
the header row off, and a row number or cell outside the table is `E122`.

## Geometry and style

| Call | Meaning |
|---|---|
| `App.move(widget, x, y)` | move to a new position |
| `App.size(widget, width, height)` | set the box size |
| `App.color(widget, "white", "#1F3864")` | text colour and background colour (either may be left out) |
| `App.font(widget, "Padauk", 16)` | font family and size |
| `App.show(widget)` / `App.hide(widget)` | show / hide |
| `App.enable(widget)` / `App.disable(widget)` | grey out and stop interaction, or restore it |

Colours are names (`red`, `green`, `blue`, `yellow`, `orange`, `purple`,
`pink`, `brown`, `gray`, `lightgray`, `darkgray`, `cyan`, `magenta`, `navy`,
`teal`, `lime`, `olive`, `maroon`, `silver`, `gold`, `skyblue`, `black`,
`white`) or hex strings (`"#1F3864"`, `"#abc"`).

**Myanmar text**: fonts are chosen by name from the system, so `Padauk`,
`Myanmar Text`, `Pyidaungsu` and `Noto Sans Myanmar` are recognised when
installed. The first Myanmar-capable system font found is also added as a
fallback for every widget, so labels and inputs show Myanmar text even
without a `App.font` call. A name that isn't available only changes the
font size; the widget keeps a readable font instead of failing.

## Events

| Call | Meaning |
|---|---|
| `App.on_key(w, "Enter", "on_enter")` | call a function when a key is pressed while the window is running. Names: `Enter`, `Escape`, `Space`, `Tab`, `Backspace`, `Delete`, `Insert`, `Home`, `End`, `PageUp`, `PageDown`, `Up`/`Down`/`Left`/`Right` (also `ArrowUp`, ...), `F1`–`F12`, `a`–`z`, `0`–`9`, `+`, `-`, `/`, `.`, `,` |
| `App.every(w, 1000, "tick")` | call a function every N milliseconds (Tkinter's `after`), for clocks, animations and polling |

While a text box has keyboard focus, only `Enter`, `Escape`, `Tab` and the
arrow keys still reach `App.on_key` — otherwise a shortcut like `on_key(w,
"a", ...)` would fight with ordinary typing. Typing `a` into an input also
triggers that binding when no text box is focused.

A callback that raises an error stops that `App.run(w)` call, so the error
message reaches the program (and can be caught with `စမ်းရန် / ဖမ်းပါ`).

## Dialogs

| Call | Meaning |
|---|---|
| `App.message("Saved!")` | popup with an OK button |
| `yes = App.ask("Delete this?")` | Yes/No popup; `True` when Yes is chosen |
| `path = App.pick_file()` | the system's file-open dialog; the chosen path as text, or `""` when cancelled |

Dialogs are native windows, so they work before or during `App.run`.
`App.message` and `App.ask` are modal: the window behind them waits until
they are dismissed.

## Drawing on a canvas

| Call | Meaning |
|---|---|
| `App.rect(cv, 10, 10, 80, 40, "red")` | filled rectangle at `x, y`, `w` × `h` |
| `App.circle(cv, 60, 30, 12, "#1F3864")` | filled circle at `x, y` with radius `r` |
| `App.line(cv, 0, 0, 120, 60, "teal")` | line from (`x1`, `y1`) to (`x2`, `y2`) |
| `App.text(cv, 4, 4, "hi", "black")` | text at `x, y` (top-left corner) |
| `App.clear(cv)` | erase everything drawn so far |

Coordinates are relative to the canvas's own top-left corner. Calls stack in
order, so later shapes draw on top. A common animation pattern is
`App.clear(cv)` followed by redrawing from an `App.every` callback.

## A complete program

```
# A tiny calculator: two inputs, four buttons, one label for the result.
နည်းပညာများ App ကို အသုံးပြုပါ။

w အတွက် App ၏ screen(320, 220) ကို လုပ်ပါ။
App ၏ title(w, "Akkhara Calculator") ကို လုပ်ပါ။

a အတွက် App ၏ input(w, 20, 20, 120) ကို လုပ်ပါ။
b အတွက် App ၏ input(w, 160, 20, 120) ကို လုပ်ပါ။

plus အတွက် App ၏ button(w, "＋", 20, 60, "on_plus") ကို လုပ်ပါ။
minus အတွက် App ၏ button(w, "－", 100, 60, "on_minus") ကို လုပ်ပါ။
quit အတွက် App ၏ button(w, "Quit", 180, 60, "on_quit") ကို လုပ်ပါ။

result အတွက် App ၏ label(w, "= ?", 20, 110) ကို လုပ်ပါ။
App ၏ font(result, "Padauk", 18) ကို လုပ်ပါ။

fn on_plus() {
    x :float = App.number(a);
    y :float = App.number(b);
    App.set(result, x + y);
}

fn on_minus() {
    x :float = App.number(a);
    y :float = App.number(b);
    App.set(result, x - y);
}

fn on_quit() {
    App.close(w);
}

App ၏ run(w) ကို လုပ်ပါ။
```

`App.number(a)` reads each box as a number (an empty box counts as `0`), so
typing numbers into the two boxes and clicking `＋` shows the sum.
[`examples/app_calculator.akk`](../../examples/app_calculator.akk) is a
runnable version of the same program, and
[`examples/app_paint.akk`](../../examples/app_paint.akk) draws an animation
on a canvas.

## Errors

| Code | Cause |
|---|---|
| `E086` | no such function in the `App` library |
| `E087` | wrong number of arguments |
| `E089` | `App` was used without `နည်းပညာများ App ကို အသုံးပြုပါ။` |
| `E090` | a coordinate, size, row/column index or interval argument wasn't a number |
| `E120` | a window/widget handle was expected, something else was given |
| `E121` | the handle has no window or widget behind it |
| `E122` | the widget kind doesn't support that: `get`/`set` on a canvas, `items` on a label, `set` a choice/listbox to an item it doesn't have, `cell`/`set_cell` on a non-table, a row/cell that doesn't exist, `rect`/`circle`/... on a non-canvas, an unreadable image file, an empty `choice` list |
| `E123` | `App.run` couldn't start the event loop, or a second `App.run` was started while one was still running |
| `E124` | an unknown key name for `on_key`, or an interval below 1 ms for `every` |
| `E125` | an unknown colour name or a malformed hex colour |
| `E126` | a text argument was expected (a callback name, a colour string, `App.number` on a checkbox, `App.set_items` with something that isn't a list, ...) |
| `E127` | a callback fired but no function with that name exists |
| `E128` | `App.number` was given text that isn't a number |

All of them work with `စမ်းရန် / ဖမ်းပါ`:

```
စမ်းရန်
    v အတွက် App ၏ get(cv) ကို လုပ်ပါ။
E122 ကို ဖမ်းပါ။
    "canvas မှာ get မရပါ" ကို ဖော်ပြပါ။
ပြီး။
```

## Limits and notes

- **Windows and Linux** run the GUI on its own thread. On **macOS** winit
  insists that the event loop owns the main thread, so `App.run` reports
  `E123` there instead of opening a window.
- Only one `App.run` can be active at a time; other open windows are shown
  as extra OS windows, but their buttons/keys/timers wait until their own
  `App.run` is called.
- `App.run(w)` returns when window `w` closes, including when it is closed
  with `App.close(w)`. Closing the main window ends the GUI session: the
  remaining windows close with it.
- A window closed before `App.run` is a no-op: `App.run` returns at once.
- Coordinates are not scaled for high-DPI screens; sizes are in logical
  pixels.
- Images are decoded on first draw, so a very large picture appears a moment
  after the window opens.

## Files

```
main.rs     <- Rust source, compiled into the akk binary
README.md   <- this file
```
