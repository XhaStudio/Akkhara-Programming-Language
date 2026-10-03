// Akkhara App library (App)
//
// Usage (from an .akk program):
//     နည်းပညာများ App ကို အသုံးပြုပါ။
//
//     w အတွက် App ၏ screen(400, 300) ကို လုပ်ပါ။
//     App ၏ title(w, "My App") ကို လုပ်ပါ။
//     lb အတွက် App ၏ label(w, "Name:", 20, 20) ကို လုပ်ပါ။
//     box အတွက် App ၏ input(w, 20, 50, 200) ကို လုပ်ပါ။
//     b အတွက် App ၏ button(w, "Save", 20, 90, "on_save") ကို လုပ်ပါ။
//     App ၏ run(w) ကို လုပ်ပါ။
//
// Design (see the library README for the full reference):
//
//   * Handles -- `screen`, `canvas` and every widget function return a
//     handle, which is a plain Akkhara object carrying an `id`. The
//     interpreter wraps the ids from this module into `Value::Object`s and
//     unwraps them again on the way back in, exactly like `request`
//     returns a response object.
//
//   * Callbacks by name -- a button (or `on_key`, or `every`) stores the
//     *name* of an Akkhara function as text. When the user clicks, this
//     module sends the name to `run`'s callback closure, which the
//     interpreter uses to call the function by name. Nothing here needs
//     to know about `Value` or `Stmt`.
//
//   * Event loop -- `run(window)` blocks on a channel until that window
//     closes, like Tkinter's `mainloop()`. The GUI itself lives on its own
//     thread (eframe/egui); the interpreter thread stays in charge of
//     running Akkhara code, and every widget mutation goes through the
//     shared `MODEL` below.
//
//   * Layout -- absolute x/y placement, like Tkinter's `place`: each
//     widget stores its own top-left corner and box size.
//
// This module is compiled into the akk binary (see the `#[path]` include
// in src/main.rs) and is registered by the interpreter when a program
// imports the App library.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Color32, FontFamily, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

// ---------------------------------------------------------------------------
// Values passed across the interpreter boundary
// ---------------------------------------------------------------------------

/// One widget value, in the shape the interpreter can translate to and from
/// its own `Value` type. Keeping this module free of `crate::interpreter`
/// types is what lets it be a standalone library, the same way `request`
/// returns its own `HttpResponse`.
#[derive(Debug, Clone, PartialEq)]
pub enum AppValue {
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl AppValue {
    /// Rendering used when a value has to be shown as text (setting an
    /// input's contents from a number, for instance).
    pub fn as_text(&self) -> String {
        match self {
            AppValue::Text(s) => s.clone(),
            AppValue::Int(i) => i.to_string(),
            AppValue::Float(f) => {
                if f.fract() == 0.0 {
                    format!("{:.1}", f)
                } else {
                    f.to_string()
                }
            }
            AppValue::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
//
// Every message starts with its own `E1xx` code so programs can catch them
// with `စမ်းရန် / ဖမ်းပါ` (or the eng `try`/`catch`) exactly like the other
// built-in libraries. The interpreter appends `(လိုင်း N)`.
// ---------------------------------------------------------------------------

fn err_not_a_handle(id: u64, want: &str) -> String {
    format!(
        "E120 App handle မှားနေပါသည် (id {})။ \"{}\" တစ်ခု လိုအပ်ပါသည်။",
        id, want
    )
}

fn err_unknown_handle(id: u64) -> String {
    format!(
        "E121 App handle (id {}) ကို ရှာမတွေ့ပါ။ window/widget ကို ဖျက်ပြီး ဖြစ်နိုင်ပါသည်။",
        id
    )
}

fn err_unsupported(kind: &str, op: &str, detail: &str) -> String {
    format!(
        "E122 \"{}\" widget သည် \"{}\" ကို မထောက်ပံ့ပါ။ {}",
        kind, op, detail
    )
}

fn err_run(detail: &str) -> String {
    format!("E123 App.run လုပ်၍မရပါ။ {}", detail)
}

fn err_bad_event(fn_name: &str, detail: &str) -> String {
    format!("E124 App.{} — {}", fn_name, detail)
}

fn err_bad_color(name: &str) -> String {
    format!(
        "E125 အရောင်နာမည် \"{}\" ကို မသိပါ။ နာမည် (red, blue, ...) သို့မဟုတ် hex (\"#1F3864\") ကို သုံးပါ။",
        name
    )
}

/// The `E126` wording for an `App.<fn>` argument that has to be text. The
/// interpreter produces this error with the source line appended; the text
/// lives here so every App error message is in one place.
#[allow(dead_code)]
fn err_want_text(fn_name: &str, want: &str) -> String {
    format!(
        "E126 \"App.{}\" ၏ argument သည် {} ဖြစ်ရပါသည်။",
        fn_name, want
    )
}

/// Raised when a callback fires but no Akkhara function with that name
/// exists (a typo in the name passed to `button` / `on_key` / `every`).
pub fn err_missing_callback(name: &str) -> String {
    format!(
        "E127 \"{}\" ဆိုသော function ကို ရှာမတွေ့သဖြင့် App callback ကို ခေါ်၍မရပါ။",
        name
    )
}

/// Raised by `App.number` when the text it read isn't a number (an input
/// box holding `"twelve"`, for instance). Empty text is not this error:
/// `App.number` counts an empty box as `0`.
pub fn err_not_a_number(text: &str) -> String {
    format!(
        "E128 \"{}\" သည် ကိန်းဂဏန်း မဟုတ်ပါ။ App.number အတွက် ကိန်းဂဏန်းစာသား (ဥပမာ \"12\", \"-3.5\") လိုအပ်ပါသည်။",
        text
    )
}

// ---------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    fn to_color32(self) -> Color32 {
        Color32::from_rgb(self.r, self.g, self.b)
    }

    fn lighten(self, amount: f32) -> Color32 {
        let f = |c: u8| -> u8 {
            let v = c as f32 + (255.0 - c as f32) * amount;
            v.clamp(0.0, 255.0) as u8
        };
        Color32::from_rgb(f(self.r), f(self.g), f(self.b))
    }

    fn darken(self, amount: f32) -> Color32 {
        let f = |c: u8| -> u8 {
            let v = c as f32 * (1.0 - amount);
            v.clamp(0.0, 255.0) as u8
        };
        Color32::from_rgb(f(self.r), f(self.g), f(self.b))
    }
}

/// Parses `"#1F3864"` / `"#abc"` / a plain colour name into a `Color`.
/// Used by `App.color` and by the canvas drawing functions.
pub fn parse_color(name: &str) -> Result<Color, String> {
    let s = name.trim();

    if let Some(hex) = s.strip_prefix('#') {
        let digits: Vec<u8> = hex
            .chars()
            .map(|c| c.to_digit(16).map(|d| d as u8).ok_or(()))
            .collect::<Result<_, ()>>()
            .map_err(|_| err_bad_color(name))?;
        return match digits.len() {
            3 => Ok(Color {
                r: digits[0] * 17,
                g: digits[1] * 17,
                b: digits[2] * 17,
            }),
            6 => Ok(Color {
                r: digits[0] * 16 + digits[1],
                g: digits[2] * 16 + digits[3],
                b: digits[4] * 16 + digits[5],
            }),
            _ => Err(err_bad_color(name)),
        };
    }

    let rgb = match s.to_ascii_lowercase().replace(' ', "").as_str() {
        "black" => (0x00, 0x00, 0x00),
        "white" => (0xFF, 0xFF, 0xFF),
        "red" => (0xE5, 0x39, 0x35),
        "green" => (0x43, 0xA0, 0x47),
        "blue" => (0x1E, 0x88, 0xE5),
        "yellow" => (0xFD, 0xD8, 0x35),
        "orange" => (0xFB, 0x8C, 0x00),
        "purple" => (0x8E, 0x24, 0xAA),
        "pink" => (0xEC, 0x40, 0x7A),
        "brown" => (0x6D, 0x4C, 0x41),
        "gray" | "grey" => (0x75, 0x75, 0x75),
        "lightgray" | "lightgrey" => (0xD4, 0xD4, 0xD4),
        "darkgray" | "darkgrey" => (0x42, 0x42, 0x42),
        "cyan" | "aqua" => (0x00, 0xAC, 0xC1),
        "magenta" => (0xD8, 0x1B, 0x60),
        "navy" => (0x1F, 0x38, 0x64),
        "teal" => (0x00, 0x83, 0x8F),
        "lime" => (0x7C, 0xB3, 0x42),
        "olive" => (0x80, 0x80, 0x00),
        "maroon" => (0x6D, 0x1C, 0x1C),
        "silver" => (0xBD, 0xBD, 0xBD),
        "gold" => (0xFF, 0xC1, 0x07),
        "skyblue" => (0x81, 0xD4, 0xFA),
        _ => return Err(err_bad_color(name)),
    };
    Ok(Color {
        r: rgb.0,
        g: rgb.1,
        b: rgb.2,
    })
}

/// Colours the GUI falls back to when a widget has no `App.color` of its own.
const DEFAULT_WINDOW_BG: Color32 = Color32::from_rgb(0xF4, 0xF4, 0xF4);
const DEFAULT_TEXT: Color32 = Color32::from_rgb(0x21, 0x21, 0x21);
const DEFAULT_FIELD_BG: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const DEFAULT_BUTTON_BG: Color32 = Color32::from_rgb(0xE0, 0xE0, 0xE0);
const DISABLED_BG: Color32 = Color32::from_rgb(0xEC, 0xEC, 0xEC);
const DISABLED_TEXT: Color32 = Color32::from_rgb(0x9E, 0x9E, 0x9E);
const BORDER: Color32 = Color32::from_rgb(0x8C, 0x8C, 0x8C);

const DEFAULT_FONT_SIZE: f32 = 14.0;

// ---------------------------------------------------------------------------
// The widget/window model
// ---------------------------------------------------------------------------

/// What a widget is. Chosen once, when the widget is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Label,
    Button,
    Input,
    Textarea,
    Checkbox,
    Choice,
    Image,
    Canvas,
}

impl Kind {
    /// The english name used in messages (`App.get` on a canvas, ...).
    pub fn name(self) -> &'static str {
        match self {
            Kind::Label => "label",
            Kind::Button => "button",
            Kind::Input => "input",
            Kind::Textarea => "textarea",
            Kind::Checkbox => "checkbox",
            Kind::Choice => "choice",
            Kind::Image => "image",
            Kind::Canvas => "canvas",
        }
    }
}

/// A widget's current value. `App.get` reads it and `App.set` writes it.
#[derive(Debug, Clone)]
enum WidgetValue {
    /// No value at all (labels, buttons, images and canvases keep their
    /// contents in `text` / `shapes` instead).
    None,
    Text(String),
    Flag(bool),
}

/// One shape drawn on a canvas, in canvas-local coordinates.
#[derive(Debug, Clone)]
enum DrawOp {
    Rect { x: f32, y: f32, w: f32, h: f32, color: Color },
    Circle { x: f32, y: f32, r: f32, color: Color },
    Line { x1: f32, y1: f32, x2: f32, y2: f32, color: Color },
    Text { x: f32, y: f32, text: String, color: Color },
}

struct Widget {
    id: u64,
    kind: Kind,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    /// Label/button caption, image path, or the text of a canvas `text()`.
    text: String,
    value: WidgetValue,
    /// `choice` items, in the order they were given.
    items: Vec<String>,
    selected: Option<usize>,
    fg: Option<Color>,
    bg: Option<Color>,
    /// A font name requested with `App.font`, resolved against the loaded
    /// Myanmar families when the widget is drawn.
    font_name: Option<String>,
    font_size: Option<f32>,
    visible: bool,
    enabled: bool,
    /// The Akkhara function name a button calls when it is clicked.
    callback: Option<String>,
    shapes: Vec<DrawOp>,
    /// Loaded image texture, kept between frames.
    texture: Option<egui::TextureHandle>,
}

impl Widget {
    fn new(id: u64, kind: Kind) -> Self {
        Widget {
            id,
            kind,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            text: String::new(),
            value: WidgetValue::None,
            items: Vec::new(),
            selected: None,
            fg: None,
            bg: None,
            font_name: None,
            font_size: None,
            visible: true,
            enabled: true,
            callback: None,
            shapes: Vec::new(),
            texture: None,
        }
    }

    fn font_size(&self) -> f32 {
        self.font_size.unwrap_or(DEFAULT_FONT_SIZE)
    }

    /// The rect this widget occupies, given the top-left corner of the
    /// window's content area.
    fn rect(&self, origin: Pos2) -> Rect {
        Rect::from_min_size(origin + Vec2::new(self.x, self.y), Vec2::new(self.w, self.h))
    }
}

/// A key binding registered with `App.on_key`.
struct KeyBinding {
    key: egui::Key,
    func: String,
}

/// A repeating timer registered with `App.every`.
struct Timer {
    interval: Duration,
    next: Instant,
    func: String,
}

struct Window {
    id: u64,
    title: String,
    width: f32,
    height: f32,
    widgets: Vec<Widget>,
    keys: Vec<KeyBinding>,
    timers: Vec<Timer>,
    /// Set when the interpreter calls `App.close`, or when the user clicks
    /// the window's close button.
    closed: bool,
    /// Whether the interpreter has already been told this window closed.
    close_reported: bool,
    /// The window an `App.run` is driving right now.
    active: bool,
    /// The title currently on the window bar, so a title is only re-sent to
    /// the backend when the program actually changes it.
    shown_title: Option<String>,
}

/// Events the GUI thread sends to whichever `App.run` is waiting.
pub enum GuiEvent {
    /// A button was clicked / a key was pressed / a timer fired.
    Callback {
        window: u64,
        func: String,
        /// The GUI waits for this after sending the event, so the window
        /// stays frozen while the Akkhara callback runs (modal, like
        /// Tkinter), and so a dialog opened from the callback can be shown
        /// without the UI racing ahead.
        ack: Sender<bool>,
    },
    /// A window closed (by the user, by `App.close`, or with the whole GUI
    /// session ending).
    Closed { window: u64 },
    /// The GUI thread's event loop ended. `error` carries a backend failure
    /// (only ever produced before the loop was running).
    Ended { error: Option<String> },
}

struct Session {
    tx: Sender<GuiEvent>,
    /// The window eframe opens as its main viewport. Any other open window
    /// becomes an extra viewport.
    root: u64,
    ended: bool,
}

struct Model {
    windows: Vec<Window>,
    /// Ids are handed out to windows and widgets from one counter, so a
    /// handle can never be mistaken for the other kind.
    next_id: AtomicU64,
    session: Option<Session>,
}

static MODEL: Mutex<Model> = Mutex::new(Model {
    windows: Vec::new(),
    next_id: AtomicU64::new(1),
    session: None,
});

/// Keeps a dropped or poisoned lock from taking the whole program down:
/// the model is a plain data structure, so recovering is safe.
fn model() -> MutexGuard<'static, Model> {
    MODEL.lock().unwrap_or_else(|e| e.into_inner())
}

/// The receiving end of the GUI thread's event channel. `App.run` takes it
/// out while it waits, and puts it back for a later call; a fresh channel is
/// created whenever a new GUI session starts.
static EVENTS: Mutex<Option<Receiver<GuiEvent>>> = Mutex::new(None);

fn events() -> MutexGuard<'static, Option<Receiver<GuiEvent>>> {
    EVENTS.lock().unwrap_or_else(|e| e.into_inner())
}

fn next_id() -> u64 {
    model().next_id.fetch_add(1, Ordering::Relaxed)
}

impl Model {
    fn window_index(&self, id: u64) -> Result<usize, String> {
        if let Some(i) = self.windows.iter().position(|w| w.id == id) {
            return Ok(i);
        }
        if self.windows.iter().any(|w| w.widgets.iter().any(|g| g.id == id)) {
            return Err(err_not_a_handle(id, "window handle"));
        }
        Err(err_unknown_handle(id))
    }

    fn window_mut(&mut self, id: u64) -> Result<&mut Window, String> {
        let i = self.window_index(id)?;
        Ok(&mut self.windows[i])
    }

    /// Locates a widget by its handle, returning `(window index, widget index)`.
    fn widget_index(&self, id: u64) -> Result<(usize, usize), String> {
        for (wi, w) in self.windows.iter().enumerate() {
            if let Some(gi) = w.widgets.iter().position(|g| g.id == id) {
                return Ok((wi, gi));
            }
        }
        if self.windows.iter().any(|w| w.id == id) {
            return Err(err_not_a_handle(id, "widget handle"));
        }
        Err(err_unknown_handle(id))
    }

    fn widget_mut(&mut self, id: u64) -> Result<&mut Widget, String> {
        let (wi, gi) = self.widget_index(id)?;
        Ok(&mut self.windows[wi].widgets[gi])
    }

    /// Finds a canvas widget, so the drawing calls can report "that is a
    /// label, not a canvas" instead of silently doing nothing.
    fn canvas_mut(&mut self, id: u64) -> Result<&mut Widget, String> {
        let widget = self.widget_mut(id)?;
        if widget.kind == Kind::Canvas {
            Ok(widget)
        } else {
            Err(err_unsupported(
                widget.kind.name(),
                "rect/circle/line/text/clear",
                "canvas handle တစ်ခု လိုအပ်ပါသည်။",
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Window and widget creation
// ---------------------------------------------------------------------------

/// `App.screen(width, height)` -- creates a window and returns its handle id.
pub fn screen(width: f32, height: f32) -> u64 {
    let id = next_id();
    let mut m = model();
    m.windows.push(Window {
        id,
        title: "Akkhara App".to_string(),
        // Keep the window inside a sane range: too-small windows cannot show
        // anything, and enormous ones are almost always a mistake in the
        // program (or a mistyped unit).
        width: width.clamp(120.0, 8000.0),
        height: height.clamp(80.0, 8000.0),
        widgets: Vec::new(),
        keys: Vec::new(),
        timers: Vec::new(),
        closed: false,
        close_reported: false,
        active: false,
        shown_title: None,
    });
    id
}

/// `App.title(window, text)`.
pub fn title(id: u64, text: &str) -> Result<(), String> {
    let mut m = model();
    m.window_mut(id)?.title = text.to_string();
    Ok(())
}

/// `App.close(window)` -- marks a window as closed. The GUI notices on its
/// next frame, tells `App.run`, and the window disappears.
pub fn close(id: u64) -> Result<(), String> {
    let mut m = model();
    let win = m.window_mut(id)?;
    if win.closed {
        return Ok(());
    }
    win.closed = true;
    Ok(())
}

/// Shared tail of every widget creator: find the window, push the widget,
/// return its handle id.
fn add_widget(window: u64, widget: Widget) -> Result<u64, String> {
    let mut m = model();
    let win = m.window_mut(window)?;
    let id = widget.id;
    win.widgets.push(widget);
    Ok(id)
}

/// `App.label(window, text, x, y)`.
pub fn label(window: u64, text: &str, x: f32, y: f32) -> Result<u64, String> {
    let mut w = Widget::new(next_id(), Kind::Label);
    w.text = text.to_string();
    w.x = x;
    w.y = y;
    w.h = DEFAULT_FONT_SIZE * 1.6;
    add_widget(window, w)
}

/// `App.button(window, text, x, y, callback)` -- `callback` is an Akkhara
/// function name; leaving it out creates a button that does nothing.
pub fn button(
    window: u64,
    text: &str,
    x: f32,
    y: f32,
    callback: Option<&str>,
) -> Result<u64, String> {
    let mut w = Widget::new(next_id(), Kind::Button);
    w.text = text.to_string();
    w.x = x;
    w.y = y;
    w.w = 110.0;
    w.h = 30.0;
    w.callback = callback.map(|s| s.to_string());
    add_widget(window, w)
}

/// `App.input(window, x, y, width)` -- one line of text.
pub fn input(window: u64, x: f32, y: f32, width: f32) -> Result<u64, String> {
    let mut w = Widget::new(next_id(), Kind::Input);
    w.x = x;
    w.y = y;
    w.w = width.max(24.0);
    w.h = 26.0;
    w.value = WidgetValue::Text(String::new());
    add_widget(window, w)
}

/// `App.textarea(window, x, y, width, height)` -- several lines of text.
pub fn textarea(window: u64, x: f32, y: f32, width: f32, height: f32) -> Result<u64, String> {
    let mut w = Widget::new(next_id(), Kind::Textarea);
    w.x = x;
    w.y = y;
    w.w = width.max(24.0);
    w.h = height.max(24.0);
    w.value = WidgetValue::Text(String::new());
    add_widget(window, w)
}

/// `App.checkbox(window, text, x, y)`.
pub fn checkbox(window: u64, text: &str, x: f32, y: f32) -> Result<u64, String> {
    let mut w = Widget::new(next_id(), Kind::Checkbox);
    w.text = text.to_string();
    w.x = x;
    w.y = y;
    w.h = 24.0;
    w.value = WidgetValue::Flag(false);
    add_widget(window, w)
}

/// `App.choice(window, [items], x, y)` -- a dropdown built from a list.
pub fn choice(window: u64, items: Vec<String>, x: f32, y: f32) -> Result<u64, String> {
    if items.is_empty() {
        return Err("E122 \"choice\" widget အတွက် item တစ်ခုထက်မကပါသော list တစ်ခု လိုအပ်ပါသည်။".to_string());
    }
    let mut w = Widget::new(next_id(), Kind::Choice);
    w.x = x;
    w.y = y;
    w.w = 150.0;
    w.h = 26.0;
    w.items = items;
    add_widget(window, w)
}

/// `App.image(window, path, x, y)` -- the file's own pixel size becomes the
/// widget's size (unless `App.size` overrides it).
pub fn image(window: u64, path: &str, x: f32, y: f32) -> Result<u64, String> {
    let (pw, ph) = image::image_dimensions(path).map_err(|e| {
        format!(
            "E122 ပုံဖိုင် \"{}\" ကို ဖတ်၍မရပါ — {} (png/jpeg/bmp/gif/webp ကို ထောက်ပံ့ပါသည်)။",
            path, e
        )
    })?;
    let mut w = Widget::new(next_id(), Kind::Image);
    w.text = path.to_string();
    w.x = x;
    w.y = y;
    w.w = pw as f32;
    w.h = ph as f32;
    add_widget(window, w)
}

/// `App.canvas(window, x, y, width, height)` -- a blank area to draw on.
pub fn canvas(window: u64, x: f32, y: f32, width: f32, height: f32) -> Result<u64, String> {
    let mut w = Widget::new(next_id(), Kind::Canvas);
    w.x = x;
    w.y = y;
    w.w = width.max(16.0);
    w.h = height.max(16.0);
    add_widget(window, w)
}

// ---------------------------------------------------------------------------
// Reading and changing widget values
// ---------------------------------------------------------------------------

/// `App.get(widget)` -- input/textarea text, a checkbox's `True`/`False`, or
/// the item a choice has selected. Labels, buttons and images report their
/// text/path; a canvas has no single value (`E122`).
pub fn get(id: u64) -> Result<AppValue, String> {
    let mut m = model();
    let widget = m.widget_mut(id)?;
    match (widget.kind, &widget.value) {
        (Kind::Canvas, _) => Err(err_unsupported(
            "canvas",
            "get",
            "canvas အတွက် get မရပါ — ပုံဆွဲခြင်းကို rect/circle/line/text ဖြင့် လုပ်ပါ။",
        )),
        (Kind::Input, WidgetValue::Text(s)) | (Kind::Textarea, WidgetValue::Text(s)) => {
            Ok(AppValue::Text(s.clone()))
        }
        (Kind::Checkbox, WidgetValue::Flag(b)) => Ok(AppValue::Bool(*b)),
        (Kind::Choice, _) => Ok(AppValue::Text(
            widget
                .selected
                .and_then(|i| widget.items.get(i))
                .cloned()
                .unwrap_or_default(),
        )),
        _ => Ok(AppValue::Text(widget.text.clone())),
    }
}

/// `App.set(widget, value)` -- changes what a widget shows. Text widgets
/// accept anything (numbers and booleans are shown as text), a checkbox
/// wants `True`/`False`, and a choice wants one of its own items.
pub fn set(id: u64, value: AppValue) -> Result<(), String> {
    let mut m = model();
    let widget = m.widget_mut(id)?;
    match widget.kind {
        Kind::Input | Kind::Textarea => {
            widget.value = WidgetValue::Text(value.as_text());
            Ok(())
        }
        Kind::Checkbox => match value {
            AppValue::Bool(b) => {
                widget.value = WidgetValue::Flag(b);
                Ok(())
            }
            _ => Err(err_unsupported(
                "checkbox",
                "set",
                "True သို့မဟုတ် False လိုအပ်ပါသည်။",
            )),
        },
        Kind::Choice => {
            let wanted = value.as_text();
            match widget.items.iter().position(|i| *i == wanted) {
                Some(i) => {
                    widget.selected = Some(i);
                    Ok(())
                }
                None => Err(err_unsupported(
                    "choice",
                    "set",
                    &format!(
                        "\"{}\" သည် ရွေးစရာစာရင်းထဲတွင် မပါပါ။ ({})",
                        wanted,
                        widget.items.join(", ")
                    ),
                )),
            }
        }
        Kind::Label | Kind::Button | Kind::Image => {
            widget.text = value.as_text();
            Ok(())
        }
        Kind::Canvas => Err(err_unsupported(
            "canvas",
            "set",
            "canvas အတွက် set မရပါ — clear(cv) ဖြင့် ရှင်းပြီး ပြန်ဆွဲပါ။",
        )),
    }
}

/// `App.move(widget, x, y)`.
pub fn move_to(id: u64, x: f32, y: f32) -> Result<(), String> {
    let mut m = model();
    let widget = m.widget_mut(id)?;
    widget.x = x;
    widget.y = y;
    Ok(())
}

/// `App.size(widget, width, height)`.
pub fn resize(id: u64, width: f32, height: f32) -> Result<(), String> {
    let mut m = model();
    let widget = m.widget_mut(id)?;
    widget.w = width.max(0.0);
    widget.h = height.max(0.0);
    Ok(())
}

/// `App.color(widget, fg, bg)` -- either colour may be left out.
pub fn color(id: u64, fg: Option<&str>, bg: Option<&str>) -> Result<(), String> {
    let fg = fg.map(parse_color).transpose()?;
    let bg = bg.map(parse_color).transpose()?;
    let mut m = model();
    let widget = m.widget_mut(id)?;
    if fg.is_some() {
        widget.fg = fg;
    }
    if bg.is_some() {
        widget.bg = bg;
    }
    Ok(())
}

/// `App.font(widget, name, size)`. A name that is not one of the loaded
/// Myanmar families only changes the size -- the widget keeps a readable
/// default font instead of failing.
pub fn font(id: u64, name: &str, size: f32) -> Result<(), String> {
    let mut m = model();
    let widget = m.widget_mut(id)?;
    widget.font_name = Some(name.to_string());
    widget.font_size = Some(size.clamp(6.0, 96.0));
    Ok(())
}

/// `App.show(widget)` / `App.hide(widget)`.
pub fn show(id: u64) -> Result<(), String> {
    let mut m = model();
    m.widget_mut(id)?.visible = true;
    Ok(())
}

pub fn hide(id: u64) -> Result<(), String> {
    let mut m = model();
    m.widget_mut(id)?.visible = false;
    Ok(())
}

/// `App.enable(widget)` / `App.disable(widget)`.
pub fn enable(id: u64) -> Result<(), String> {
    let mut m = model();
    m.widget_mut(id)?.enabled = true;
    Ok(())
}

pub fn disable(id: u64) -> Result<(), String> {
    let mut m = model();
    m.widget_mut(id)?.enabled = false;
    Ok(())
}

// ---------------------------------------------------------------------------
// Canvas drawing
// ---------------------------------------------------------------------------

pub fn rect(id: u64, x: f32, y: f32, w: f32, h: f32, color: &str) -> Result<(), String> {
    let c = parse_color(color)?;
    let mut m = model();
    m.canvas_mut(id)?.shapes.push(DrawOp::Rect {
        x,
        y,
        w,
        h,
        color: c,
    });
    Ok(())
}

pub fn circle(id: u64, x: f32, y: f32, r: f32, color: &str) -> Result<(), String> {
    let c = parse_color(color)?;
    let mut m = model();
    m.canvas_mut(id)?.shapes.push(DrawOp::Circle {
        x,
        y,
        r,
        color: c,
    });
    Ok(())
}

pub fn line(id: u64, x1: f32, y1: f32, x2: f32, y2: f32, color: &str) -> Result<(), String> {
    let c = parse_color(color)?;
    let mut m = model();
    m.canvas_mut(id)?.shapes.push(DrawOp::Line {
        x1,
        y1,
        x2,
        y2,
        color: c,
    });
    Ok(())
}

pub fn text(id: u64, x: f32, y: f32, content: &str, color: &str) -> Result<(), String> {
    let c = parse_color(color)?;
    let mut m = model();
    m.canvas_mut(id)?.shapes.push(DrawOp::Text {
        x,
        y,
        text: content.to_string(),
        color: c,
    });
    Ok(())
}

/// `App.clear(canvas)` -- forgets everything drawn so far.
pub fn clear(id: u64) -> Result<(), String> {
    let mut m = model();
    m.canvas_mut(id)?.shapes.clear();
    Ok(())
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// `App.on_key(window, key, function)` -- calls `function` (by name) whenever
/// the key is pressed while the window is running.
pub fn on_key(window: u64, key: &str, func: &str) -> Result<(), String> {
    let parsed = parse_key(key).ok_or_else(|| {
        err_bad_event(
            "on_key",
            &format!(
                "\"{}\" ဆိုသော key ကို မသိပါ။ (Enter, Escape, Space, Tab, Up/Down/Left/Right, F1-F12, a-z, 0-9, + - * / . , ကို သုံးနိုင်ပါသည်)",
                key
            ),
        )
    })?;
    let mut m = model();
    let win = m.window_mut(window)?;
    win.keys.push(KeyBinding {
        key: parsed,
        func: func.to_string(),
    });
    Ok(())
}

/// `App.every(window, milliseconds, function)` -- calls `function` every N
/// milliseconds while the window is running (Tkinter's `after`).
pub fn every(window: u64, ms: u64, func: &str) -> Result<(), String> {
    if ms == 0 {
        return Err(err_bad_event(
            "every",
            "အချိန်အပိုင်းအခြားသည် 1 မီလီစက္ကန့် အနည်းဆုံး ဖြစ်ရပါသည်။",
        ));
    }
    let mut m = model();
    let win = m.window_mut(window)?;
    win.timers.push(Timer {
        interval: Duration::from_millis(ms),
        next: Instant::now() + Duration::from_millis(ms),
        func: func.to_string(),
    });
    Ok(())
}

/// Maps the key names `App.on_key` accepts onto egui's key codes.
fn parse_key(name: &str) -> Option<egui::Key> {
    use egui::Key;
    let lower = name.trim().to_ascii_lowercase();
    let key = match lower.as_str() {
        "enter" | "return" => Key::Enter,
        "escape" | "esc" => Key::Escape,
        "space" | "spacebar" => Key::Space,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "insert" => Key::Insert,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" => Key::PageDown,
        "up" | "arrowup" => Key::ArrowUp,
        "down" | "arrowdown" => Key::ArrowDown,
        "left" | "arrowleft" => Key::ArrowLeft,
        "right" | "arrowright" => Key::ArrowRight,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        "a" => Key::A,
        "b" => Key::B,
        "c" => Key::C,
        "d" => Key::D,
        "e" => Key::E,
        "f" => Key::F,
        "g" => Key::G,
        "h" => Key::H,
        "i" => Key::I,
        "j" => Key::J,
        "k" => Key::K,
        "l" => Key::L,
        "m" => Key::M,
        "n" => Key::N,
        "o" => Key::O,
        "p" => Key::P,
        "q" => Key::Q,
        "r" => Key::R,
        "s" => Key::S,
        "t" => Key::T,
        "u" => Key::U,
        "v" => Key::V,
        "w" => Key::W,
        "x" => Key::X,
        "y" => Key::Y,
        "z" => Key::Z,
        "0" => Key::Num0,
        "1" => Key::Num1,
        "2" => Key::Num2,
        "3" => Key::Num3,
        "4" => Key::Num4,
        "5" => Key::Num5,
        "6" => Key::Num6,
        "7" => Key::Num7,
        "8" => Key::Num8,
        "9" => Key::Num9,
        "+" | "plus" => Key::Plus,
        "-" | "minus" => Key::Minus,
        "/" | "slash" => Key::Slash,
        "." | "period" | "dot" => Key::Period,
        "," | "comma" => Key::Comma,
        _ => return None,
    };
    Some(key)
}

/// Keys that still reach the window while a text box has keyboard focus.
/// Without this, typing "a" into an input would also fire an `on_key("a")`
/// binding; Enter/Escape/arrows stay useful for "submit"/"cancel" shortcuts.
fn key_works_while_typing(key: egui::Key) -> bool {
    use egui::Key::*;
    matches!(
        key,
        Enter | Escape | Tab | ArrowUp | ArrowDown | ArrowLeft | ArrowRight
    )
}

// ---------------------------------------------------------------------------
// Dialogs (native message boxes and file picker, via rfd)
// ---------------------------------------------------------------------------

/// `App.message(text)` -- a popup with an OK button.
pub fn message(text: &str) {
    rfd::MessageDialog::new()
        .set_title("Akkhara App")
        .set_description(text)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

/// `App.ask(text)` -- a Yes/No popup; `True` when Yes is chosen.
pub fn ask(text: &str) -> bool {
    matches!(
        rfd::MessageDialog::new()
            .set_title("Akkhara App")
            .set_description(text)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show(),
        rfd::MessageDialogResult::Yes
    )
}

/// `App.pick_file()` -- opens the system's file chooser and returns the
/// chosen path as text, or `""` when the dialog is cancelled.
pub fn pick_file() -> String {
    rfd::FileDialog::new()
        .pick_file()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------

/// A Myanmar-capable font that ships with the operating system, looked up in
/// order of preference. The first one that exists is loaded as a fallback
/// for the default fonts (so Myanmar text renders everywhere) and registered
/// under its own family name (so `App.font(w, "Padauk", 16)` can pick it).
fn myanmar_font_candidates() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let dirs = [
            std::env::var_os("WINDIR").map(|w| PathBuf::from(w).join("Fonts")),
            std::env::var_os("LOCALAPPDATA")
                .map(|w| PathBuf::from(w).join("Microsoft").join("Windows").join("Fonts")),
        ];
        for dir in dirs.into_iter().flatten() {
            out.push(("Myanmar Text".to_string(), dir.join("mmrtext.ttf")));
            out.push(("Padauk".to_string(), dir.join("Padauk-Regular.ttf")));
            out.push(("Padauk".to_string(), dir.join("Padauk.ttf")));
            out.push(("Pyidaungsu".to_string(), dir.join("pyidaungsu.ttf")));
            out.push((
                "Noto Sans Myanmar".to_string(),
                dir.join("NotoSansMyanmar-Regular.ttf"),
            ));
        }
    }

    #[cfg(target_os = "macos")]
    {
        out.push((
            "Myanmar Sangam MN".to_string(),
            PathBuf::from("/System/Library/Fonts/Supplemental/MyanmarSangamMN.ttf"),
        ));
        out.push((
            "Noto Sans Myanmar".to_string(),
            PathBuf::from("/Library/Fonts/NotoSansMyanmar-Regular.ttf"),
        ));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        out.push((
            "Noto Sans Myanmar".to_string(),
            PathBuf::from("/usr/share/fonts/truetype/noto/NotoSansMyanmar-Regular.ttf"),
        ));
        out.push((
            "Padauk".to_string(),
            PathBuf::from("/usr/share/fonts/truetype/padauk/Padauk-Regular.ttf"),
        ));
        out.push((
            "Noto Sans Myanmar".to_string(),
            PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansMyanmar-Regular.otf"),
        ));
        out.push((
            "Myanmar Text".to_string(),
            PathBuf::from("/usr/share/fonts/truetype/myanmar/NotoSansMyanmar-Regular.ttf"),
        ));
    }

    out.retain(|(_, path)| path.exists());
    out
}

/// Family names the GUI actually loaded, so `App.font` can tell a real
/// family from an unknown name. Filled in when the GUI starts; empty before
/// that, which makes every name resolve to the default family.
static LOADED_FAMILIES: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn install_fonts(ctx: &egui::Context) {
    let mut definitions = egui::FontDefinitions::default();
    let mut loaded = Vec::new();

    for (family, path) in myanmar_font_candidates() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let font_name = format!("akk-myanmar-{}", loaded.len());
        definitions
            .font_data
            .insert(font_name.clone(), Arc::new(egui::FontData::from_owned(bytes)));

        // Fallback for the default families: Latin text keeps the bundled
        // font, Myanmar text is picked up from this one. (Fallbacks apply
        // per-glyph, so this never overrides the primary font's shapes.)
        for default_family in [FontFamily::Proportional, FontFamily::Monospace] {
            definitions
                .families
                .entry(default_family)
                .or_default()
                .push(font_name.clone());
        }

        // And under its own name, for `App.font(widget, "Padauk", 16)`.
        let named = if loaded.iter().any(|f| f == &family) {
            format!("{} {}", family, loaded.len())
        } else {
            family.clone()
        };
        definitions
            .families
            .entry(FontFamily::Name(named.clone().into()))
            .or_default()
            .push(font_name);

        loaded.push(named);
    }

    ctx.set_fonts(definitions);
    *LOADED_FAMILIES.lock().unwrap_or_else(|e| e.into_inner()) = loaded;
}

/// Picks the font family for a widget: the requested `App.font` name when
/// that family really was loaded, otherwise the default proportional family
/// (so an unknown font name can never panic egui).
fn family_for(widget: &Widget) -> FontFamily {
    if let Some(name) = &widget.font_name {
        let families = LOADED_FAMILIES.lock().unwrap_or_else(|e| e.into_inner());
        let lower = name.to_ascii_lowercase();
        if let Some(found) = families
            .iter()
            .find(|f| f.to_ascii_lowercase() == lower)
            .cloned()
        {
            return FontFamily::Name(found.into());
        }
    }
    FontFamily::Proportional
}

fn font_id(widget: &Widget) -> FontId {
    FontId::new(widget.font_size(), family_for(widget))
}

// ---------------------------------------------------------------------------
// The GUI: one eframe app driving every open window
// ---------------------------------------------------------------------------

/// What the GUI wants the interpreter (or the backend) to be told about,
/// collected while painting and acted on once the model lock is released.
enum Action {
    Callback { window: u64, func: String },
    /// Tell the waiting `App.run` that this window is gone.
    ReportClosed { window: u64 },
}

struct GuiApp {
    /// The window eframe opened as its main viewport.
    root: u64,
}

impl eframe::App for GuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut actions: Vec<Action> = Vec::new();

        // The root window fills eframe's main viewport. The `Ui` eframe
        // hands over has no background of its own, which is exactly what an
        // absolutely-placed layout wants.
        paint_window(ui, self.root, &mut actions);

        // Every other open window gets its own OS window (an egui "deferred
        // viewport"). egui falls back to drawing them as panels inside the
        // main window when the platform has no multi-window support.
        let others: Vec<u64> = {
            let m = model();
            m.windows
                .iter()
                .filter(|w| w.id != self.root && !w.closed)
                .map(|w| w.id)
                .collect()
        };
        for id in others {
            let (title, size) = {
                let m = model();
                match m.windows.iter().find(|w| w.id == id) {
                    Some(w) => (w.title.clone(), Vec2::new(w.width, w.height)),
                    None => continue,
                }
            };
            let viewport_id = egui::ViewportId::from_hash_of(format!("akk-app-window-{}", id));
            ui.ctx().show_viewport_deferred(
                viewport_id,
                egui::ViewportBuilder::default()
                    .with_title(title)
                    .with_inner_size(size)
                    .with_min_inner_size(Vec2::new(160.0, 100.0)),
                move |ui, _class| {
                    let mut actions = Vec::new();
                    paint_window(ui, id, &mut actions);
                    run_actions(&mut actions);
                },
            );
        }

        run_actions(&mut actions);
    }
}

/// Applies the window's requested title and closes it if the model says so,
/// then paints its widgets.
fn paint_window(ui: &mut egui::Ui, window_id: u64, actions: &mut Vec<Action>) {
    let mut m = model();
    let Some(win_index) = m.windows.iter().position(|w| w.id == window_id) else {
        return;
    };

    // The user clicked the window's close button.
    if ui.input(|i| i.viewport().close_requested()) && !m.windows[win_index].closed {
        m.windows[win_index].closed = true;
    }

    let closed = m.windows[win_index].closed;
    if closed {
        let root = m.windows[win_index].id == m.session.as_ref().map(|s| s.root).unwrap_or(0);
        if root {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            let viewport_id =
                egui::ViewportId::from_hash_of(format!("akk-app-window-{}", window_id));
            ui.ctx()
                .send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Close);
        }
        let win = &mut m.windows[win_index];
        if !win.close_reported {
            win.close_reported = true;
            actions.push(Action::ReportClosed { window: window_id });
        }
        return;
    }

    // Keep the window bar in sync if the program re-titles a running window.
    let title = m.windows[win_index].title.clone();
    if m.windows[win_index].shown_title.as_deref() != Some(title.as_str()) {
        m.windows[win_index].shown_title = Some(title.clone());
        if m.session.as_ref().map(|s| s.root) == Some(window_id) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(title));
        } else {
            let viewport_id =
                egui::ViewportId::from_hash_of(format!("akk-app-window-{}", window_id));
            ui.ctx()
                .send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Title(title));
        }
    }

    let active = m.windows[win_index].active;
    let origin = ui.max_rect().min;
    let clip = ui.max_rect().shrink(1.0);
    ui.painter().rect_filled(clip, 0.0, DEFAULT_WINDOW_BG);
    let painter = ui.painter().clone();

    // Timers first: a due timer becomes a callback, and the frame asks for a
    // repaint when the next one is scheduled.
    let now = Instant::now();
    let mut next_wake: Option<Duration> = None;
    {
        let win = &mut m.windows[win_index];
        for timer in win.timers.iter_mut() {
            if timer.next <= now {
                if active {
                    actions.push(Action::Callback {
                        window: window_id,
                        func: timer.func.clone(),
                    });
                }
                // Skipping straight to the next future tick keeps a slow
                // frame from firing the same timer several times at once.
                while timer.next <= now {
                    timer.next += timer.interval;
                }
            }
            if active {
                let wait = timer.next.saturating_duration_since(now);
                next_wake = Some(next_wake.map_or(wait, |w| w.min(wait)));
            }
        }
    }
    if let Some(wait) = next_wake {
        ui.ctx().request_repaint_after(wait);
    }

    let win = &mut m.windows[win_index];
    let widgets = &mut win.widgets;

    // Keyboard bindings -- but not the keys the user is typing into a text
    // box, so an `on_key("a")` shortcut does not fight with text entry.
    if active && !win.keys.is_empty() {
        let typing = ui.memory(|m| m.focused().is_some());
        let pressed: Vec<String> = win
            .keys
            .iter()
            .filter(|b| {
                !typing || key_works_while_typing(b.key)
            })
            .filter(|b| ui.input(|i| i.key_pressed(b.key)))
            .map(|b| b.func.clone())
            .collect();
        for func in pressed {
            actions.push(Action::Callback {
                window: window_id,
                func,
            });
        }
    }

    for widget in widgets.iter_mut() {
        if !widget.visible {
            continue;
        }
        paint_widget(ui, &painter, origin, window_id, widget, active, actions);
    }
}

/// Draws one widget and records any interaction it produced.
#[allow(clippy::too_many_arguments)]
fn paint_widget(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    origin: Pos2,
    window_id: u64,
    widget: &mut Widget,
    active: bool,
    actions: &mut Vec<Action>,
) {
    let rect = widget.rect(origin);
    let font = font_id(widget);
    let id = egui::Id::new(("akk-app-widget", widget.id));
    let fg = widget
        .fg
        .map(Color::to_color32)
        .unwrap_or(DEFAULT_TEXT);
    let fg = if widget.enabled { fg } else { DISABLED_TEXT };

    match widget.kind {
        Kind::Label => {
            painter.text(rect.min, Align2::LEFT_TOP, &widget.text, font, fg);
        }

        Kind::Button => {
            let response = ui.interact(rect, id, Sense::click());
            let base = widget.bg.unwrap_or(Color {
                r: DEFAULT_BUTTON_BG.r(),
                g: DEFAULT_BUTTON_BG.g(),
                b: DEFAULT_BUTTON_BG.b(),
            });
            let fill = if !widget.enabled {
                DISABLED_BG
            } else if response.is_pointer_button_down_on() {
                base.darken(0.12)
            } else if response.hovered() {
                base.lighten(0.15)
            } else {
                base.to_color32()
            };
            painter.rect_filled(rect, 4.0, fill);
            painter.rect_stroke(rect, 4.0, Stroke::new(1.0, BORDER), StrokeKind::Inside);
            painter.text(rect.center(), Align2::CENTER_CENTER, &widget.text, font, fg);
            if widget.enabled && response.clicked() && active {
                if let Some(func) = widget.callback.clone() {
                    actions.push(Action::Callback {
                        window: window_id,
                        func,
                    });
                }
            }
        }

        Kind::Input | Kind::Textarea => {
            let is_multiline = widget.kind == Kind::Textarea;
            let font_size = widget.font_size();
            let bg = widget
                .bg
                .map(Color::to_color32)
                .unwrap_or(DEFAULT_FIELD_BG);
            let WidgetValue::Text(value) = &mut widget.value else {
                return;
            };
            painter.rect_filled(rect, 3.0, bg);
            painter.rect_stroke(rect, 3.0, Stroke::new(1.0, BORDER), StrokeKind::Inside);

            let mut edit = if is_multiline {
                egui::TextEdit::multiline(value)
            } else {
                egui::TextEdit::singleline(value)
            };
            edit = edit
                .id(id)
                .font(font)
                .text_color(fg)
                .background_color(Color32::TRANSPARENT)
                .frame(egui::Frame::NONE)
                .desired_width(rect.width().max(12.0))
                .margin(egui::Margin::symmetric(4, 2));
            if is_multiline {
                let rows = ((rect.height() / (font_size * 1.3)).floor() as usize).max(2);
                edit = edit.desired_rows(rows);
            }
            if !widget.enabled {
                edit = edit.interactive(false);
            }
            ui.put(rect, edit);
        }

        Kind::Checkbox => {
            let box_size = (widget.font_size() * 1.2).clamp(13.0, 28.0);
            let box_rect = Rect::from_min_size(
                rect.min + Vec2::new(0.0, (rect.height() - box_size) * 0.5),
                Vec2::splat(box_size),
            );
            let response = ui.interact(box_rect, id, Sense::click());
            if widget.enabled && response.clicked() && active {
                if let WidgetValue::Flag(flag) = &mut widget.value {
                    *flag = !*flag;
                }
            }
            let checked = matches!(widget.value, WidgetValue::Flag(true));
            painter.rect_filled(box_rect, 2.0, DISABLED_BG);
            painter.rect_stroke(box_rect, 2.0, Stroke::new(1.0, BORDER), StrokeKind::Inside);
            if checked {
                let check = if widget.enabled { fg } else { DISABLED_TEXT };
                let a = box_rect.min + Vec2::new(box_size * 0.20, box_size * 0.52);
                let b = box_rect.min + Vec2::new(box_size * 0.42, box_size * 0.76);
                let c = box_rect.min + Vec2::new(box_size * 0.80, box_size * 0.24);
                painter.line_segment([a, b], Stroke::new(2.0, check));
                painter.line_segment([b, c], Stroke::new(2.0, check));
            }
            painter.text(
                rect.min + Vec2::new(box_size + 8.0, rect.height() * 0.5),
                Align2::LEFT_CENTER,
                &widget.text,
                font,
                fg,
            );
        }

        Kind::Choice => {
            let items = widget.items.clone();
            let selected = widget.selected;
            let selected_text = selected
                .and_then(|i| items.get(i).cloned())
                .unwrap_or_default();
            let mut new_selected = selected;
            let bg = widget
                .bg
                .map(Color::to_color32)
                .unwrap_or(DEFAULT_FIELD_BG);
            let combo_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), rect.height()));
            ui.scope_builder(egui::UiBuilder::new().max_rect(combo_rect), |ui| {
                ui.visuals_mut().widgets.inactive.bg_fill = bg;
                egui::ComboBox::from_id_salt(id)
                    .width(combo_rect.width())
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        for (i, item) in items.iter().enumerate() {
                            ui.selectable_value(&mut new_selected, Some(i), item);
                        }
                    });
            });
            if active && new_selected != selected {
                widget.selected = new_selected;
            }
        }

        Kind::Image => {
            if widget.texture.is_none() {
                if let Ok(loaded) = image::open(&widget.text) {
                    let rgba = loaded.to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    let color_image =
                        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                    widget.texture = Some(ui.ctx().load_texture(
                        format!("akk-app-image-{}", widget.id),
                        color_image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            match &widget.texture {
                Some(texture) => {
                    let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
                    painter.image(texture.id(), rect, uv, Color32::WHITE);
                }
                None => {
                    painter.rect_filled(rect, 3.0, DISABLED_BG);
                    painter.rect_stroke(rect, 3.0, Stroke::new(1.0, BORDER), StrokeKind::Inside);
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        format!("? {}", widget.text),
                        font,
                        DISABLED_TEXT,
                    );
                }
            }
        }

        Kind::Canvas => {
            let bg = widget
                .bg
                .map(Color::to_color32)
                .unwrap_or(DEFAULT_FIELD_BG);
            painter.rect_filled(rect, 0.0, bg);
            painter.rect_stroke(rect, 0.0, Stroke::new(1.0, BORDER), StrokeKind::Inside);
            let canvas_painter = painter.with_clip_rect(rect);
            let origin = rect.min;
            for shape in &widget.shapes {
                match shape {
                    DrawOp::Rect { x, y, w, h, color } => {
                        let r = Rect::from_min_size(
                            origin + Vec2::new(*x, *y),
                            Vec2::new(*w, *h),
                        );
                        canvas_painter.rect_filled(r, 0.0, color.to_color32());
                    }
                    DrawOp::Circle { x, y, r, color } => {
                        canvas_painter.circle_filled(
                            origin + Vec2::new(*x, *y),
                            *r,
                            color.to_color32(),
                        );
                    }
                    DrawOp::Line {
                        x1,
                        y1,
                        x2,
                        y2,
                        color,
                    } => {
                        canvas_painter.line_segment(
                            [
                                origin + Vec2::new(*x1, *y1),
                                origin + Vec2::new(*x2, *y2),
                            ],
                            Stroke::new(1.0, color.to_color32()),
                        );
                    }
                    DrawOp::Text { x, y, text, color } => {
                        canvas_painter.text(
                            origin + Vec2::new(*x, *y),
                            Align2::LEFT_TOP,
                            text,
                            font.clone(),
                            color.to_color32(),
                        );
                    }
                }
            }
        }
    }
}

/// Sends the collected actions to the interpreter. If the interpreter is not
/// currently running this window, the callback is dropped (the window is
/// only interactive while `App.run` is driving it), but a closure is always
/// reported.
fn run_actions(actions: &mut Vec<Action>) {
    let tx = {
        let m = model();
        match &m.session {
            Some(session) => session.tx.clone(),
            None => return,
        }
    };
    for action in actions.drain(..) {
        match action {
            Action::Callback { window, func } => {
                let active = {
                    let m = model();
                    m.windows
                        .iter()
                        .find(|w| w.id == window)
                        .map(|w| w.active)
                        .unwrap_or(false)
                };
                if !active {
                    continue;
                }
                let (ack_tx, ack_rx) = channel();
                if tx
                    .send(GuiEvent::Callback {
                        window,
                        func,
                        ack: ack_tx,
                    })
                    .is_err()
                {
                    return;
                }
                // Wait for the callback to finish: the window stays frozen
                // while Akkhara code runs, and a dialog opened from that
                // code is modal. `App.close` from inside the callback is
                // picked up on the next frame.
                match ack_rx.recv_timeout(Duration::from_secs(120)) {
                    Ok(true) | Err(_) => {}
                    Ok(false) => {} // the callback failed; run() unwinds and closes us
                }
            }
            Action::ReportClosed { window } => {
                let _ = tx.send(GuiEvent::Closed { window });
            }
        }
    }
}

/// Body of the GUI thread: runs eframe until the main window closes (or the
/// program asks the session to end), then marks every window closed and
/// reports back. Panics are caught too, so a backend that refuses to start
/// (`App.run` on macOS, where winit insists on the main thread) becomes an
/// `E123` the program can see instead of a hang.
fn gui_thread(root: u64, tx: Sender<GuiEvent>) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| gui_session(root)));

    let error = match outcome {
        Ok(None) => None,
        Ok(Some(e)) => Some(e),
        Err(_) => Some(
            "E123 GUI thread ရပ်တန့်သွားပါသည်။ (ဒီစနစ်တွင် window ကို အခြား thread မှ ဖွင့်၍မရပါ)"
                .to_string(),
        ),
    };

    // The session is over: every window goes with it.
    let mut closed_ids = Vec::new();
    {
        let mut m = model();
        for w in m.windows.iter_mut() {
            if !w.closed || !w.close_reported {
                closed_ids.push(w.id);
            }
            w.closed = true;
            w.close_reported = true;
            w.active = false;
        }
        if let Some(session) = &mut m.session {
            session.ended = true;
        }
    }
    for id in closed_ids {
        let _ = tx.send(GuiEvent::Closed { window: id });
    }
    let _ = tx.send(GuiEvent::Ended { error });
}

/// Runs eframe, returning the backend error when it fails to start.
fn gui_session(root: u64) -> Option<String> {
    let (title, size) = {
        let m = model();
        match m.windows.iter().find(|w| w.id == root) {
            Some(w) => (w.title.clone(), Vec2::new(w.width, w.height)),
            None => ("Akkhara App".to_string(), Vec2::new(400.0, 300.0)),
        }
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(size)
            .with_min_inner_size(Vec2::new(160.0, 100.0))
            .with_app_id("akk-app"),
        renderer: eframe::Renderer::Glow,
        event_loop_builder: Some(Box::new(event_loop_hook)),
        ..Default::default()
    };

    match eframe::run_native(
        "Akkhara App",
        options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            cc.egui_ctx.set_visuals(egui::Visuals::light());
            Ok(Box::new(GuiApp { root }))
        }),
    ) {
        Ok(()) => None,
        Err(e) => Some(format!(
            "E123 GUI window ကို ဖွင့်၍မရပါ - {} (ဂရပ်ဖစ် driver မရှိသော စက်များတွင် App ကို မသုံးနိုင်ပါ)",
            e
        )),
    }
}

/// winit refuses to create an event loop off the main thread unless it is
/// told to, and the GUI thread here never is the main thread (the interpreter
/// thread stays inside `App.run`). Every platform with an escape hatch gets
/// it; macOS has none, so `App.run` reports `E123` there.
fn event_loop_hook(builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>) {
    #[cfg(target_os = "windows")]
    {
        use winit::platform::windows::EventLoopBuilderExtWindows as _;
        builder.with_any_thread(true);
    }
    #[cfg(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    {
        use winit::platform::wayland::EventLoopBuilderExtWayland as _;
        builder.with_any_thread(true);
        use winit::platform::x11::EventLoopBuilderExtX11 as _;
        builder.with_any_thread(true);
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd"
    )))]
    {
        let _ = builder;
    }
}

// ---------------------------------------------------------------------------
// The event loop: App.run(window)
// ---------------------------------------------------------------------------

/// `App.run(window)` -- shows the window and blocks until it closes, calling
/// `on_callback` (which the interpreter backs with a by-name function call)
/// whenever a button is clicked, a key binding fires, or a timer is due.
pub fn run(window: u64, on_callback: &mut dyn FnMut(&str) -> Result<(), String>) -> Result<(), String> {
    // Start (or reuse) the GUI session, and make this window the one whose
    // callbacks are live.
    {
        let mut m = model();
        let index = m.window_index(window)?;
        if m.windows[index].closed {
            return Ok(());
        }
        if m.windows.iter().any(|w| w.active) {
            return Err(err_run("App.run တစ်ခု လုပ်ဆောင်နေဆဲ ဖြစ်ပါသည်။"));
        }
        m.windows[index].active = true;

        let session_alive = m.session.as_ref().map(|s| !s.ended).unwrap_or(false);
        if !session_alive {
            let (tx, rx) = channel();
            m.session = Some(Session {
                tx: tx.clone(),
                root: window,
                ended: false,
            });
            *events() = Some(rx);
            // Release the model before the GUI thread takes its first frame.
            drop(m);
            let spawned = thread::Builder::new()
                .name("akk-app-gui".to_string())
                .spawn(move || gui_thread(window, tx));
            if let Err(e) = spawned {
                let mut m = model();
                m.session = None;
                *events() = None;
                return Err(err_run(&format!("GUI thread ကို ဖွင့်၍မရပါ - {}", e)));
            }
        }
    }

    let rx = events()
        .take()
        .unwrap_or_else(|| {
            let (_tx, rx) = channel();
            rx
        });
    let mut failure: Option<String> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(GuiEvent::Callback { window: w, func, ack }) => {
                if w != window {
                    let _ = ack.send(true);
                    continue;
                }
                match on_callback(&func) {
                    Ok(()) => {
                        let _ = ack.send(true);
                    }
                    Err(e) => {
                        let _ = ack.send(false);
                        failure = Some(e);
                        break;
                    }
                }
            }
            Ok(GuiEvent::Closed { window: w }) => {
                if w == window {
                    break;
                }
            }
            Ok(GuiEvent::Ended { error }) => {
                match error {
                    Some(e) => failure = Some(e),
                    None => {}
                }
                break;
            }
            Err(RecvTimeoutError::Timeout) => {
                let ended = model()
                    .session
                    .as_ref()
                    .map(|s| s.ended)
                    .unwrap_or(true);
                if ended {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    // Leave the session ready for a later `App.run` -- the windows that are
    // still open keep their GUI thread, and events for them queue up in the
    // channel until their own run call listens. A session that has ended
    // (its main window closed) drops its channel, so the next `App.run`
    // starts a fresh one.
    {
        let mut m = model();
        if let Some(index) = m.windows.iter().position(|w| w.id == window) {
            m.windows[index].active = false;
        }
        let alive = m.session.as_ref().map(|s| !s.ended).unwrap_or(false);
        drop(m);
        if alive {
            *events() = Some(rx);
        }
    }

    match failure {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
