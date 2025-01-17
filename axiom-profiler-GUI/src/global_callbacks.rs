use std::{ops::Deref, rc::Rc, sync::Mutex};
use yew::{html, html::Scope, prelude::{Context, Html}, Callback, Children, Component, ContextProvider, DragEvent, MouseEvent, Properties};

// Public interface

pub trait GlobalCallbacksContext {
    fn get_callbacks_registerer(&self) -> Option<Rc<GlobalCallbacks>>;
}
impl<T: Component> GlobalCallbacksContext for Scope<T> {
    fn get_callbacks_registerer(&self) -> Option<Rc<GlobalCallbacks>> {
        self.context(Callback::noop()).map(|c| c.0)
    }
}

pub struct GlobalCallbacks {
    pub register_mouse_move: CallbackRegisterer<MouseEvent>,
    pub register_mouse_up: CallbackRegisterer<MouseEvent>,
    pub register_mouse_out: CallbackRegisterer<MouseEvent>,
    pub register_drag_over: CallbackRegisterer<DragEvent>,
}
impl PartialEq for GlobalCallbacks {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

pub struct CallbackRegisterer<T: 'static>(Box<dyn Fn(Callback<T>) -> CallbackRef>);
impl<T> Deref for CallbackRegisterer<T> {
    type Target = Box<dyn Fn(Callback<T>) -> CallbackRef>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for CallbackRef {
    fn drop(&mut self) {
        self.0();
    }

}
pub struct CallbackRef(Box<dyn Fn()>);

pub struct GlobalCallbacksProvider {
    mouse_move: CallbackHolder<MouseEvent>,
    mouse_up: CallbackHolder<MouseEvent>,
    mouse_out: CallbackHolder<MouseEvent>,
    drag_over: CallbackHolder<DragEvent>,

    registerer: Rc<GlobalCallbacks>,
}

#[derive(Properties, PartialEq)]
pub struct GlobalCallbacksProviderProps {
    pub children: Children,
}

// Private

impl CallbackRegisterer<MouseEvent> {
    fn new_mouse(link: Scope<GlobalCallbacksProvider>, kind: MouseEventKind) -> Self {
        let id = Mutex::<usize>::new(0);
        Self(Box::new(move |callback| {
            let mut id = id.lock().unwrap();
            let id_v = *id;
            *id += 1;
            drop(id);
            link.send_message(Msg::RegisterMouse(kind, id_v, callback));
            let link = link.clone();
            CallbackRef(Box::new(move || link.send_message(Msg::DeRegisterMouse(kind, id_v))))
        }))
    }
}
impl CallbackRegisterer<DragEvent> {
    fn new_drag(link: Scope<GlobalCallbacksProvider>, kind: DragEventKind) -> Self {
        let id = Mutex::<usize>::new(0);
        Self(Box::new(move |callback| {
            let mut id = id.lock().unwrap();
            let id_v = *id;
            *id += 1;
            drop(id);
            link.send_message(Msg::RegisterDrag(kind, id_v, callback));
            let link = link.clone();
            CallbackRef(Box::new(move || link.send_message(Msg::DeRegisterDrag(kind, id_v))))
        }))
    }
}

impl GlobalCallbacksProvider {
    fn get_mouse_mut(&mut self, kind: MouseEventKind) -> &mut CallbackHolder<MouseEvent> {
        match kind {
            MouseEventKind::MouseMove => &mut self.mouse_move,
            MouseEventKind::MouseUp => &mut self.mouse_up,
            MouseEventKind::MouseOut => &mut self.mouse_out,
        }
    }
    fn get_drag_mut(&mut self, kind: DragEventKind) -> &mut CallbackHolder<DragEvent> {
        match kind {
            DragEventKind::DragOver => &mut self.drag_over,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum MouseEventKind {
    MouseMove,
    MouseUp,
    MouseOut,
}


#[derive(Debug, Copy, Clone)]
pub enum DragEventKind {
    DragOver,
}

pub enum Msg {
    RegisterMouse(MouseEventKind, usize, Callback<MouseEvent>),
    DeRegisterMouse(MouseEventKind, usize),
    OnMouse(MouseEventKind, MouseEvent),

    RegisterDrag(DragEventKind, usize, Callback<DragEvent>),
    DeRegisterDrag(DragEventKind, usize),
    OnDrag(DragEventKind, DragEvent),
}

struct CallbackHolder<T>(Vec<(usize, Callback<T>)>);
impl<T> Default for CallbackHolder<T> {
    fn default() -> Self {
        Self(Vec::default())
    }
}

impl Component for GlobalCallbacksProvider {
    type Message = Msg;
    type Properties = GlobalCallbacksProviderProps;

    fn create(ctx: &Context<Self>) -> Self {
        let registerer = GlobalCallbacks {
            register_mouse_move: CallbackRegisterer::new_mouse(ctx.link().clone(), MouseEventKind::MouseMove),
            register_mouse_up: CallbackRegisterer::new_mouse(ctx.link().clone(), MouseEventKind::MouseUp),
            register_mouse_out: CallbackRegisterer::new_mouse(ctx.link().clone(), MouseEventKind::MouseOut),
            register_drag_over: CallbackRegisterer::new_drag(ctx.link().clone(), DragEventKind::DragOver),
        };
        Self {
            mouse_move: CallbackHolder::default(),
            mouse_up: CallbackHolder::default(),
            mouse_out: CallbackHolder::default(),
            drag_over: CallbackHolder::default(),

            registerer: Rc::new(registerer)
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::RegisterMouse(kind, id, cb) =>
                self.get_mouse_mut(kind).0.push((id, cb)),
            Msg::DeRegisterMouse(kind, id) => {
                let cbh = self.get_mouse_mut(kind);
                let idx = cbh.0.iter().position(|(i, _)| *i == id).unwrap();
                cbh.0.swap_remove(idx);
            }
            Msg::OnMouse(kind, ev) => for (_, cb) in &self.get_mouse_mut(kind).0 {
                cb.emit(ev.clone());
            },
            Msg::RegisterDrag(kind, id, cb) =>
                self.get_drag_mut(kind).0.push((id, cb)),
            Msg::DeRegisterDrag(kind, id) => {
                let cbh = self.get_drag_mut(kind);
                let idx = cbh.0.iter().position(|(i, _)| *i == id).unwrap();
                cbh.0.swap_remove(idx);
            }
            Msg::OnDrag(kind, ev) => for (_, cb) in &self.get_drag_mut(kind).0 {
                cb.emit(ev.clone());
            },
        }
        false
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let onmousemove = ctx.link().callback(|ev: MouseEvent| Msg::OnMouse(MouseEventKind::MouseMove, ev));
        let onmouseup = ctx.link().callback(|ev: MouseEvent| Msg::OnMouse(MouseEventKind::MouseUp, ev));
        let onmouseout = ctx.link().callback(|ev: MouseEvent| Msg::OnMouse(MouseEventKind::MouseOut, ev));
        let ondragover = ctx.link().callback(|ev: DragEvent| Msg::OnDrag(DragEventKind::DragOver, ev));
        html! {
            <div style="position=absolute; top: 0; left: 0; width: 100%; height: 100%" {onmousemove} {onmouseup} {onmouseout} {ondragover}>
                <ContextProvider<Rc<GlobalCallbacks>> context={self.registerer.clone()}>
                    {ctx.props().children.clone()}
                </ContextProvider<Rc<GlobalCallbacks>>>
            </div>
        }
    }
}
