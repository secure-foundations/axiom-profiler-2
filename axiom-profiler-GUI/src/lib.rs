use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock, RwLock};

use gloo_file::File;
use gloo_file::{callbacks::FileReader, FileList};
use results::svg_result::{Msg as SVGMsg, RenderingState, SVGResult};
use smt_log_parser::items::{InstIdx, QuantIdx};
use smt_log_parser::parsers::z3::inst_graph::InstGraph;
use smt_log_parser::parsers::z3::z3parser::Z3Parser;
use smt_log_parser::parsers::{AsyncBufferRead, LogParser, ParseState, ReaderState};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_streams::ReadableStream;
use wasm_timer::Instant;
use web_sys::{HtmlElement, HtmlInputElement};
use yew::prelude::*;
use yew_router::prelude::*;
use material_yew::{MatButton, MatIcon, MatIconButton, MatDialog, WeakComponentLink};
use material_yew::dialog::{ActionType, MatDialogAction};

use crate::filters::FiltersState;
use crate::infobars::{SidebarSectionHeader, Topbar};

pub use global_callbacks::{GlobalCallbacksProvider, CallbackRef, GlobalCallbacksContext};

pub mod results;
mod utils;
mod infobars;
mod filters;
mod global_callbacks;

const SIZE_NAMES: [&'static str; 5] = ["B", "KB", "MB", "GB", "TB"];

pub static MOUSE_POSITION: OnceLock<RwLock<PagePosition>> = OnceLock::new();
pub fn mouse_position() -> &'static RwLock<PagePosition> {
    MOUSE_POSITION.get_or_init(|| RwLock::new(PagePosition { x: 0, y: 0 }))
}
#[derive(Debug, Clone, Copy)]
pub struct PagePosition {
    pub x: i32,
    pub y: i32,
}

pub static PREVENT_DEFAULT_DRAG_OVER: OnceLock<Mutex<bool>> = OnceLock::new();

pub enum Msg {
    File(Option<File>),
    LoadedFile(String, u64, Z3Parser, ParseState, bool),
    LoadingState(LoadingState),
    SelectedInsts(Vec<(InstIdx, Option<QuantIdx>)>),
    SearchMatchingLoops,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoadingState {
    NoFileSelected,
    ReadingToString,
    StartParsing,
    Parsing(ParseProgress, Callback<()>),
    // Stopped early, cancelled?
    DoneParsing(bool, bool),
    Rendering(RenderingState, bool, bool),
    FileDisplayed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseProgress {
    reader: ReaderState,
    file_size: u64,
    time: Instant,
    bytes_delta: Option<usize>,
    time_delta: Option<std::time::Duration>,
    speed: Option<f64>,
}
impl ParseProgress {
    pub fn new(reader: ReaderState, file_size: u64) -> Self {
        Self {
            reader,
            file_size,
            time: Instant::now(),
            bytes_delta: None,
            time_delta: None,
            speed: None,
        }
    }
    pub fn delta(&mut self, old: &Self) {
        assert!(self.reader.bytes_read > old.reader.bytes_read);
        if self.reader.bytes_read < old.reader.bytes_read {
            *self = old.clone();
            return;
        }
        // Value >= 0.0, the higher the value the more smoothed out the speed is
        // (but also takes longer to react to changes in speed)
        const SPEED_SMOOTHING: f64 = 10.0;
        let bytes_delta = self.reader.bytes_read - old.reader.bytes_read;
        self.bytes_delta = Some(bytes_delta);
        let time_delta = self.time - old.time;
        self.time_delta = Some(time_delta);
        let speed = bytes_delta as f64 / time_delta.as_secs_f64();
        self.speed = Some(old.speed.map(|old| (speed + SPEED_SMOOTHING * old) / (SPEED_SMOOTHING + 1.0)).unwrap_or(speed));
    }
}

#[derive(Clone)]
pub struct OpenedFileInfo {
    file_name: String,
    file_size: u64,
    parser: RcParser,
    parser_state: ParseState,
    parser_cancelled: bool,
    update: Rc<RefCell<Result<Callback<SVGMsg>, Vec<SVGMsg>>>>,
    selected_insts: Vec<(InstIdx, Option<QuantIdx>)>,
}

impl PartialEq for OpenedFileInfo {
    fn eq(&self, other: &Self) -> bool {
        self.file_name == other.file_name
            && self.file_size == other.file_size
            && self.parser == other.parser
            && std::mem::discriminant(&self.parser_state) == std::mem::discriminant(&other.parser_state)
            && self.selected_insts == other.selected_insts
    }
}

impl OpenedFileInfo {
    pub fn send_update(&self, msg: SVGMsg) {
        match &mut *self.update.borrow_mut() {
            Ok(cb) => cb.emit(msg),
            Err(e) => e.push(msg),
        }
    }
    pub fn send_updates(&self, msgs: impl Iterator<Item = SVGMsg>) {
        match &mut *self.update.borrow_mut() {
            Ok(cb) => for msg in msgs {
                cb.emit(msg);
            },
            Err(e) => e.extend(msgs),
        }
    }
}

pub struct FileDataComponent {
    file_select: NodeRef,
    file: Option<OpenedFileInfo>,
    reader: Option<FileReader>,
    pending_ops: usize,
    progress: LoadingState,
    cancel: Rc<RefCell<bool>>,
    callback_refs: [CallbackRef; 2],
}

impl Component for FileDataComponent {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let registerer = ctx.link().get_callbacks_registerer().unwrap();
        let mouse_move_ref = (registerer.register_mouse_move)(Callback::from(|event: MouseEvent| {
            *mouse_position().write().unwrap() = PagePosition { x: event.client_x(), y: event.client_y() };
        }));
        let pd = PREVENT_DEFAULT_DRAG_OVER.get_or_init(|| Mutex::default());
        let drag_over_ref = (registerer.register_drag_over)(Callback::from(|event: DragEvent| {
            *mouse_position().write().unwrap() = PagePosition { x: event.client_x(), y: event.client_y() };
            if *pd.lock().unwrap() {
                event.prevent_default();
            }
        }));
        let callback_refs = [mouse_move_ref, drag_over_ref];
        Self {
            file_select: NodeRef::default(),
            file: None,
            reader: None,
            pending_ops: 0,
            progress: LoadingState::NoFileSelected,
            cancel: Rc::default(),
            callback_refs,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        self.file.as_mut().map(|f| {
            let graph = f.parser.graph.borrow();
            f.parser.graph_loaded = graph.is_some();
            f.parser.found_mls = graph.as_ref().and_then(|g| g.found_matching_loops());
        });
        match msg {
            Msg::File(file) => {
                let Some(file) = file else {
                    return false;
                };
                let changed = self.file.is_some() || self.reader.is_some();
                drop(self.file.take());
                drop(self.reader.take());

                let file_name = file.name();
                let file_size = file.size();
                log::info!("Selected file \"{file_name}\"");
                *self.cancel.borrow_mut() = false;
                let cancel = self.cancel.clone();
                let cancel_cb = Callback::from(move |_| {
                    *cancel.borrow_mut() = true;
                });
                let cancel = self.cancel.clone();
                // Turn into stream
                let blob: &web_sys::Blob = file.as_ref();
                let stream = ReadableStream::from_raw(blob.stream().unchecked_into());
                match stream.try_into_async_read() {
                    Ok(stream) => {
                        let link = ctx.link().clone();
                        link.send_message(Msg::LoadingState(LoadingState::StartParsing));
                        let mut parser = Z3Parser::from_async(stream.buffer());
                        wasm_bindgen_futures::spawn_local(async move {
                            log::info!("Parsing \"{file_name}\"");
                            let finished = parser.process_until(|_, state| {
                                if state.lines_read % 100_000 == 0 {
                                    let parsing = ParseProgress::new(state, file_size);
                                    link.send_message(Msg::LoadingState(LoadingState::Parsing(parsing, cancel_cb.clone())));
                                }
                                !*cancel.borrow() && state.bytes_read <= 1024 * 1024 * 1024
                            }).await;
                            if finished.is_timeout() && !*cancel.borrow() {
                                // TODO: make this clear in the UI
                                log::info!("Stopped parsing at 1GB");
                            }
                            let cancel = *cancel.borrow();
                            link.send_message(Msg::LoadingState(LoadingState::DoneParsing(finished.is_timeout(), cancel)));
                            link.send_message(Msg::LoadedFile(file_name, file_size, parser.take_parser(), finished, cancel))
                        });
                    }
                    Err((_err, _stream)) => {
                        let link = ctx.link().clone();
                        link.send_message(Msg::LoadingState(LoadingState::ReadingToString));
                        let reader = gloo_file::callbacks::read_as_bytes(&file, move |res| {
                            log::info!("Loading to string \"{file_name}\"");
                            let text_data =
                                String::from_utf8(res.expect("failed to read file")).unwrap();
                            log::info!("Parsing \"{file_name}\"");
                            link.send_message(Msg::LoadingState(LoadingState::StartParsing));
                            let mut parser = Z3Parser::from_str(&text_data);
                            let finished = parser.process_until(|_, state| {
                                if state.lines_read % 100_000 == 0 {
                                    let parsing = ParseProgress::new(state, file_size);
                                    link.send_message(Msg::LoadingState(LoadingState::Parsing(parsing, cancel_cb.clone())));
                                }
                                !*cancel.borrow() && state.bytes_read <= 512 * 1024 * 1024
                            });
                            if finished.is_timeout() && !*cancel.borrow() {
                                // TODO: make this clear in the UI
                                log::info!("Stopped parsing at 0.5GB (use Chrome or Firefox to increase this limit to 1GB)");
                            }
                            let cancel = *cancel.borrow();
                            link.send_message(Msg::LoadingState(LoadingState::DoneParsing(finished.is_timeout(), cancel)));
                            link.send_message(Msg::LoadedFile(file_name, file_size, parser.take_parser(), finished, cancel))
                        });
                        self.reader = Some(reader);
                    }
                };
                changed
            }
            Msg::LoadingState(mut state) => {
                log::info!("New state \"{state:?}\"");
                if let (LoadingState::Parsing(parsing, _), LoadingState::Parsing(old, _)) = (&mut state, &self.progress) {
                    parsing.delta(old);
                }
                self.progress = state;
                true
            }
            Msg::LoadedFile(file_name, file_size, parser, parser_state, parser_cancelled) => {
                log::info!("Processing \"{file_name}\"");
                drop(self.reader.take());
                let file = OpenedFileInfo {
                    file_name,
                    file_size,
                    parser: RcParser::new(parser),
                    parser_state,
                    parser_cancelled,
                    update: Rc::new(RefCell::new(Err(Vec::new()))),
                    selected_insts: Vec::new(),
                };
                self.file = Some(file);
                true
            }
            Msg::SelectedInsts(insts) => {
                if let Some(file) = &mut self.file {
                    file.selected_insts = insts;
                    true
                } else {
                    false
                }
            }
            Msg::SearchMatchingLoops => {
                if let Some(file) = &mut self.file {
                    if let Some(g) = file.parser.graph.borrow_mut().as_mut() {
                        file.parser.found_mls = Some(g.search_matching_loops());
                        return true;
                    }
                }
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        // Parse the timestamp at compile time
        let timestamp =
            chrono::DateTime::parse_from_rfc3339(env!("VERGEN_GIT_COMMIT_TIMESTAMP")).unwrap();
        // Format using https://docs.rs/chrono/latest/chrono/format/strftime/index.html
        let version_info = format!(
            "{} ({})",
            env!("VERGEN_GIT_DESCRIBE"),
            timestamp.format("%H:%M %-d %b %y")
        );
        let version_link = format!(
            "https://github.com/viperproject/axiom-profiler-2/tree/{}",
            env!("VERGEN_GIT_SHA")
        );

        let sidebar = NodeRef::default();
        let scrollable_dialog_link: WeakComponentLink<MatDialog> = WeakComponentLink::default();

        let current_trace = match &self.file {
            Some(file) => {
                let search_matching_loops = ctx.link().callback(|_| Msg::SearchMatchingLoops);
                html!{
                    <FiltersState file={file.clone()} search_matching_loops={search_matching_loops}/>
                }
            }
            None => html!{},
        };

        // Callbacks
        let file_select_ref = self.file_select.clone();
        let on_change = ctx.link().callback(move |_| {
            let files = file_select_ref.cast::<HtmlInputElement>().unwrap().files();
            Msg::File(files.map(FileList::from).and_then(|files|
                (files.len() == 1).then(|| files[0].clone())
            ))
        });
        let sidebar_ref = sidebar.clone();
        let open_files = self.file.is_some();
        let hide_sidebar = Callback::from(move |_| {
            let sidebar = sidebar_ref.cast::<HtmlElement>().unwrap();
            if sidebar.class_list().contains("hide-sidebar") || open_files {
                let _ = sidebar.class_list().toggle("hide-sidebar");
            }
        });
        let scrollable_dialog_link_clone = scrollable_dialog_link.clone();
        let show_shortcuts = Callback::from(move |click: MouseEvent| {
            click.prevent_default();
            scrollable_dialog_link_clone.show();
        });
        let page = self.file.as_ref().map(|f| {
            let (timeout, cancel) = (f.parser_state.is_timeout(), f.parser_cancelled);
            let progress = ctx.link().callback(move |rs| match rs {
                Some(rs) => Msg::LoadingState(LoadingState::Rendering(rs, timeout, cancel)),
                None => Msg::LoadingState(LoadingState::FileDisplayed),
            });
            let selected_insts_cb = ctx.link().callback(Msg::SelectedInsts);
            Self::view_file(f.clone(), progress, selected_insts_cb)
        });
        html! {
<>
    <nav class="sidebar" ref={sidebar}>
        <header class="stable"><img src="html/logo_side_small.png" class="brand"/><div class="sidebar-button" onclick={hide_sidebar}><MatIconButton icon="menu"></MatIconButton></div></header>
        <input type="file" ref={&self.file_select} class="trace_file" accept=".log" onchange={on_change} multiple=false/>
        <div class="sidebar-scroll"><div class="sidebar-scroll-container">
            <SidebarSectionHeader header_text="Navigation" collapsed_text="Open or record a new trace"><ul>
                <li><a href="#" draggable="false" id="open_trace_file"><div class="material-icons"><MatIcon>{"folder_open"}</MatIcon></div>{"Open trace file"}</a></li>
            </ul></SidebarSectionHeader>
            {current_trace}
            <SidebarSectionHeader header_text="Support" collapsed_text="Documentation & Bugs"><ul>
                <li><a href="#" draggable="false" onclick={show_shortcuts} id="keyboard_shortcuts"><div class="material-icons"><MatIcon>{"help"}</MatIcon></div>{"Keyboard shortcuts"}</a></li>
                <li><a href="https://github.com/viperproject/axiom-profiler-2/blob/main/README.md" target="_blank" id="documentation"><div class="material-icons"><MatIcon>{"find_in_page"}</MatIcon></div>{"Documentation"}</a></li>
                <li><a href="#" draggable="false" id="flags"><div class="material-icons"><MatIcon>{"emoji_flags"}</MatIcon></div>{"Flags"}</a></li>
                <li><a href="https://github.com/viperproject/axiom-profiler-2/issues/new" target="_blank" id="report_a_bug"><div class="material-icons"><MatIcon>{"bug_report"}</MatIcon></div>{"Report a bug"}</a></li>
            </ul></SidebarSectionHeader>
            <div class="sidebar-footer">
                <div title="Number of pending operations" class="dbg-info-square"><div>{"OPS"}</div><div>{self.pending_ops}</div></div>
                <div title="Service Worker: Serving from cache. Ready for offline use" class="dbg-info-square amber"><div>{"SW"}</div><div>{"NA"}</div></div>
                <div class="version"><a href={version_link} title="Channel: stable" target="_blank">{version_info}</a></div>
            </div>
        </div></div>
    </nav>
    <div class="topbar">
        <Topbar progress={self.progress.clone()} />
    </div>
    <div class="alerts"></div>
    <div class="page">
        {page}
    </div>

    // Shortcuts dialog
    <section tabindex="0">
        <MatDialog heading={"Axiom Profiler Help"} dialog_link={scrollable_dialog_link}>
            {"There are currently no keyboard shortcuts available."}
            <MatDialogAction action_type={ActionType::Primary} action={"close"}>
                <MatButton label="Close" />
            </MatDialogAction>
        </MatDialog>
    </section>
</>
        }
    }

    fn rendered(&mut self, _ctx: &Context<Self>, first_render: bool) {
        if first_render {
            // Do this instead of `onclick` when creating `open_trace_file`
            // above. Otherwise we run into the error here:
            // https://github.com/leptos-rs/leptos/issues/2104 due to the `.click()`.
            let input = self.file_select.cast::<HtmlInputElement>().unwrap();
            let closure: Closure<dyn Fn(MouseEvent)> = Closure::new(move |e: MouseEvent| {
                e.prevent_default(); input.click();
            });
            let div = gloo::utils::document().get_element_by_id("open_trace_file").unwrap();
            div.add_event_listener_with_callback(
                "click",
                closure.as_ref().unchecked_ref(),
            ).unwrap();
            closure.forget();
        }
    }
}

impl FileDataComponent {
    fn view_file(data: OpenedFileInfo, progress: Callback<Option<RenderingState>>, selected_insts_cb: Callback<Vec<(InstIdx, Option<QuantIdx>)>>) -> Html {
        log::debug!("Viewing file");
        html! {
            <SVGResult file={data} progress={progress} selected_insts_cb={selected_insts_cb}/>
        }
    }
}

#[function_component(App)]
pub fn app() -> Html {
    html! {
        <main><GlobalCallbacksProvider> <FileDataComponent/> </GlobalCallbacksProvider></main>
    }
}

#[function_component(Test)]
fn test() -> Html {
    html! {
        <div>
        <h1>{"test"}</h1>
        </div>
    }
}

#[derive(Routable, Clone, PartialEq)]
enum Route {
    #[at("/")]
    App,
    #[at("/test")]
    Test,
}

pub struct RcParser {
    parser: Rc<RefCell<Z3Parser>>,
    graph: Rc<RefCell<Option<InstGraph>>>,
    graph_loaded: bool,
    found_mls: Option<usize>,
}

impl std::ops::Deref for RcParser {
    type Target = RefCell<Z3Parser>;

    fn deref(&self) -> &Self::Target {
        &self.parser
    }
}

impl Clone for RcParser {
    fn clone(&self) -> Self {
        Self {
            parser: self.parser.clone(),
            graph: self.graph.clone(),
            graph_loaded: self.graph_loaded,
            found_mls: self.found_mls,
        }
    }
}

impl PartialEq for RcParser {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(&*self.parser, &*other.parser)
        && self.graph_loaded == other.graph_loaded
        && self.found_mls == other.found_mls
    }
}

impl RcParser {
    fn new(parser: Z3Parser) -> Self {
        Self {
            parser: Rc::new(RefCell::new(parser)),
            graph: Rc::default(),
            graph_loaded: false,
            found_mls: None,
        }
    }
}
