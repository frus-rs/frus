// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Scoped state** (milestone 825): a [`Model`] put in scope for a subtree by [`Scoped`], and
//! read by the components below it with [`BuildContext::watch`],
//! [`BuildContext::select`] and [`BuildContext::read`].
//!
//! A value in scope is found by the components **below** the widget that put it there, the
//! nearest one first. Two instances of the same screen each put their own model in scope, and
//! each one's components find their own. A value provided for the whole application with
//! [`StateStore::provide`](crate::StateStore::provide) is the outermost scope: it is found when
//! no `Scoped` above has one of the type.
//!
//! A component that **watches** a model is built again when the model changes, and the
//! components that do not read it are not: the change marks its readers, and the next frame
//! builds those, where they stand in the tree it kept (milestone 822). A component that
//! **selects** a part of it is built again only when that part changes.
//!
//! ```
//! use frus_widgets::{column, text, Button, BuildContext, Model, Scoped, Widget, component};
//!
//! #[derive(Default)]
//! struct Cart {
//!     items: Vec<String>,
//! }
//!
//! // Built again when the number of items changes, and only then.
//! fn badge(cx: &BuildContext) -> Box<dyn Widget> {
//!     let count = cx.select(|cart: &Cart| cart.items.len());
//!     Box::new(text(format!("{count} in the cart")))
//! }
//!
//! // Never built again by the cart: it only changes it.
//! fn add(cx: &BuildContext) -> Box<dyn Widget> {
//!     let cart = cx.read::<Cart>();
//!     Box::new(Button::new("Add a pear").on_press(move || {
//!         cart.update(|cart| cart.items.push("pear".into()))
//!     }))
//! }
//!
//! // Each shop owns its cart and puts it in scope for what it shows.
//! fn shop(cx: &BuildContext) -> Box<dyn Widget> {
//!     let cart = cx.use_model(Cart::default);
//!     Box::new(Scoped::new(cart, column![component(badge), component(add)]))
//! }
//!
//! let two_shops = column![component(shop), component(shop)];
//! # let _ = two_shops;
//! ```

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use crate::notifier::{ChangeNotifier, Listenable, Subscription};
use crate::runtime::Runtime;
use crate::widget::Widget;

// ---------------------------------------------------------------------------------------
// A value in scope
// ---------------------------------------------------------------------------------------

/// One value a [`Scoped`] puts in scope for its subtree, with its type. What
/// [`Widget::scoped_values`] hands the walks.
#[derive(Clone)]
pub struct ScopedValue {
    kind: TypeId,
    name: &'static str,
    value: Rc<dyn Any>,
}

impl ScopedValue {
    /// `value`, to be found by its type.
    pub fn new<T: 'static>(value: T) -> Self {
        Self {
            kind: TypeId::of::<T>(),
            name: std::any::type_name::<T>(),
            value: Rc::new(value),
        }
    }

    /// The value, if it is a `T`.
    pub(crate) fn get<T: 'static>(&self) -> Option<Rc<T>> {
        if self.kind != TypeId::of::<T>() {
            return None;
        }
        self.value.clone().downcast::<T>().ok()
    }
}

impl std::fmt::Debug for ScopedValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ScopedValue").field(&self.name).finish()
    }
}

/// The values in scope where a walk is, innermost last. It lives in the
/// [`StateStore`](crate::StateStore): every walk that builds components holds the values of
/// the widgets it is under.
#[derive(Default)]
pub(crate) struct Scopes {
    values: RefCell<Vec<ScopedValue>>,
}

impl Scopes {
    /// The nearest value of type `T` in scope.
    pub(crate) fn find<T: 'static>(&self) -> Option<Rc<T>> {
        self.values.borrow().iter().rev().find_map(ScopedValue::get)
    }

    /// Puts `values` in scope until the returned guard drops.
    pub(crate) fn enter(&self, values: impl IntoIterator<Item = ScopedValue>) -> ScopeGuard<'_> {
        let mut held = self.values.borrow_mut();
        let from = held.len();
        held.extend(values);
        ScopeGuard { scopes: self, from }
    }

    /// Everything in scope where the walk is now, outermost first: what an overlay declared
    /// here takes with it, to be laid out and painted after the walk has left (milestone 854).
    /// `None`, and nothing allocated, where nothing is in scope.
    pub(crate) fn capture(&self) -> Option<Rc<[ScopedValue]>> {
        let values = self.values.borrow();
        (!values.is_empty()).then(|| values.iter().cloned().collect())
    }

    /// Makes `values` the whole of what is in scope until the returned guard drops, which puts
    /// back what was there: an overlay is walked under the scope it was declared in, and only
    /// that one.
    pub(crate) fn restore(&self, values: &[ScopedValue]) -> RestoreGuard<'_> {
        let before = std::mem::replace(&mut *self.values.borrow_mut(), values.to_vec());
        RestoreGuard {
            scopes: self,
            before,
        }
    }
}

/// Holds a captured scope: dropping it puts back what was in scope before.
pub(crate) struct RestoreGuard<'s> {
    scopes: &'s Scopes,
    before: Vec<ScopedValue>,
}

impl Drop for RestoreGuard<'_> {
    fn drop(&mut self) {
        *self.scopes.values.borrow_mut() = std::mem::take(&mut self.before);
    }
}

/// Holds values in scope: dropping it puts back what was in scope before.
pub(crate) struct ScopeGuard<'s> {
    scopes: &'s Scopes,
    from: usize,
}

impl Drop for ScopeGuard<'_> {
    fn drop(&mut self) {
        self.scopes.values.borrow_mut().truncate(self.from);
    }
}

/// The values `widget` puts in scope for its subtree, held for as long as the guard is: what
/// each walk that may build a component does at every node, as it does for the surface and
/// the theme. `None`, and nothing allocated, for every widget but a [`Scoped`].
#[inline]
pub(crate) fn hold<'r, Msg>(
    widget: &dyn Widget<Msg>,
    runtime: &'r Runtime,
) -> Option<ScopeGuard<'r>> {
    let mut values = Vec::new();
    widget.scoped_values(&mut values);
    (!values.is_empty()).then(|| runtime.states.scopes().enter(values))
}

// ---------------------------------------------------------------------------------------
// The widget
// ---------------------------------------------------------------------------------------

/// Puts a value in scope for its subtree: the components below find it by its type, the
/// nearest one first, with [`BuildContext::watch`](crate::BuildContext::watch),
/// [`select`](crate::BuildContext::select), [`read`](crate::BuildContext::read) or
/// [`service`](crate::BuildContext::service).
///
/// It is transparent: it adds no box, and the layout is its child's.
///
/// The value is usually a [`Model`], made where its owner is — a component's
/// [`use_model`](crate::BuildContext::use_model), a `State`, the application — and handed
/// down here. Anything `'static` can be put in scope, a plain configuration value included;
/// only a `Model` tells its readers when it changes.
///
/// ```
/// use frus_widgets::{text, Model, Scoped};
///
/// let theme_name = Model::new(String::from("ocean"));
/// let page: Scoped = Scoped::new(theme_name.clone(), text("…"));
/// # let _ = page;
/// ```
pub struct Scoped<Msg = crate::callback::Callback> {
    value: ScopedValue,
    inner: Box<dyn Widget<Msg>>,
}

impl<Msg> Scoped<Msg> {
    /// Puts `value` in scope for `child` and everything below it.
    pub fn new<T: 'static>(value: T, child: impl Widget<Msg> + 'static) -> Self {
        Self {
            value: ScopedValue::new(value),
            inner: Box::new(child),
        }
    }

    /// A scope is not a box: the child's own, unchanged.
    fn restyle(&self, base: frus_layout::Style) -> frus_layout::Style {
        base
    }
}

crate::transparent::forward_transparent!(expand Scoped {
    /// The child is built with the value in scope: a component it builds directly, which the
    /// walk reaches through this one, finds it.
    fn expand(
        &self,
        id: crate::interaction::WidgetId,
        runtime: &crate::runtime::Runtime,
        theme: &crate::theme::Theme,
    ) {
        let _scope = runtime.states.scopes().enter([self.value.clone()]);
        self.inner.expand(id, runtime, theme)
    }

    /// This one's value, then those of a `Scoped` inside, which is nearer.
    fn scoped_values(&self, out: &mut Vec<ScopedValue>) {
        out.push(self.value.clone());
        self.inner.scoped_values(out)
    }

    fn key(&self) -> Option<u64> {
        self.inner.key()
    }
    fn positioned(&self) -> Option<crate::positioned::Positioning> {
        self.inner.positioned()
    }
    fn theme_override(
        &self,
        inherited: &crate::theme::Theme,
    ) -> Option<Box<crate::theme::Theme>> {
        self.inner.theme_override(inherited)
    }
    fn media_override(&self, inherited: crate::MediaQuery) -> Option<crate::MediaQuery> {
        self.inner.media_override(inherited)
    }
    fn scaffold_override(&self) -> Option<crate::ScaffoldInfo> {
        self.inner.scaffold_override()
    }
    fn scroll_behavior_override(&self) -> Option<crate::ScrollBehavior> {
        self.inner.scroll_behavior_override()
    }
    fn autofill_group(&self) -> bool {
        self.inner.autofill_group()
    }
    fn area_toolbar(
        &self,
        context: crate::ToolbarContext,
    ) -> Option<Option<&dyn crate::widget::Widget<Msg>>> {
        self.inner.area_toolbar(context)
    }
});

// ---------------------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------------------

/// A component that read a model while it was built: its mark, and what it read.
pub(crate) struct Reader {
    /// The component's mark, the one its own state sets (milestone 822).
    pub(crate) mark: Rc<Cell<bool>>,
    /// The rebuild the component was last built in, kept up to date by its hooks.
    pub(crate) built: Rc<Cell<u64>>,
    /// The rebuild it is being built in now.
    pub(crate) epoch: u64,
}

/// Says whether the part a reader selected changed.
type PartChanged<T> = Box<dyn Fn(&T) -> bool>;

/// What one component depends on in a model.
struct Dependency<T> {
    mark: Weak<Cell<bool>>,
    /// When the component was last built: a later build that did not read the model again
    /// no longer depends on it.
    built: Weak<Cell<u64>>,
    /// The build that read it.
    epoch: u64,
    /// The whole value: any change builds it again.
    whole: bool,
    /// Parts of it: each says whether the part it selected changed.
    parts: Vec<PartChanged<T>>,
}

struct Shared<T> {
    value: RefCell<T>,
    /// The components that read it, by their mark.
    readers: RefCell<HashMap<*const Cell<bool>, Dependency<T>>>,
    /// Who listens from outside the tree.
    listeners: ChangeNotifier,
}

/// A value shared by several components, which tells the ones that **read** it when it
/// changes: they are built again, and nothing else is.
///
/// Put it in scope with [`Scoped`] (or provide it for the whole application), and read it from
/// a component with [`BuildContext::watch`](crate::BuildContext::watch) (built again on every
/// change), [`BuildContext::select`](crate::BuildContext::select) (built again when the part it
/// selects changes) or [`BuildContext::read`](crate::BuildContext::read) (never built again:
/// for a handler that only changes it).
///
/// Cloning gives another handle to the same value. It is a [`Listenable`] as well, for what
/// listens from outside the tree.
///
/// ```
/// use frus_widgets::Model;
///
/// let count = Model::new(0);
/// count.update(|n| *n += 1);
/// assert_eq!(count.get(), 1);
/// ```
pub struct Model<T> {
    shared: Rc<Shared<T>>,
}

impl<T> Clone for Model<T> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

impl<T: 'static> Model<T> {
    /// A model holding `value`.
    pub fn new(value: T) -> Self {
        Self {
            shared: Rc::new(Shared {
                value: RefCell::new(value),
                readers: RefCell::new(HashMap::new()),
                listeners: ChangeNotifier::new(),
            }),
        }
    }

    /// A copy of the value.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.shared.value.borrow().clone()
    }

    /// Reads the value without copying it.
    pub fn with<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        read(&self.shared.value.borrow())
    }

    /// Replaces the value and tells its readers — unless it is the value already there.
    pub fn set(&self, value: T)
    where
        T: PartialEq,
    {
        if *self.shared.value.borrow() == value {
            return;
        }
        *self.shared.value.borrow_mut() = value;
        self.changed();
    }

    /// Changes the value in place and tells its readers.
    pub fn update(&self, change: impl FnOnce(&mut T)) {
        change(&mut self.shared.value.borrow_mut());
        self.changed();
    }

    /// Whether two handles are the same model.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.shared, &other.shared)
    }

    /// How many components are reading it now. For tests and diagnostics.
    pub fn reader_count(&self) -> usize {
        self.shared
            .readers
            .borrow()
            .values()
            .filter(|dependency| dependency.mark.strong_count() > 0)
            .count()
    }

    /// The component `reader` read it while it was built: the whole value with `part`
    /// `None`, or one part of it.
    pub(crate) fn add_reader(&self, reader: Reader, part: Option<PartChanged<T>>) {
        let mut readers = self.shared.readers.borrow_mut();
        let dependency = readers
            .entry(Rc::as_ptr(&reader.mark))
            .or_insert_with(|| Dependency {
                mark: Rc::downgrade(&reader.mark),
                built: Rc::downgrade(&reader.built),
                epoch: reader.epoch,
                whole: false,
                parts: Vec::new(),
            });
        // What it read in an earlier build no longer holds: this build says it afresh. A mark
        // that has gone and whose address was reused is someone else's.
        if dependency.epoch != reader.epoch
            || !std::ptr::eq(dependency.mark.as_ptr(), Rc::as_ptr(&reader.mark))
            || dependency.mark.strong_count() == 0
        {
            *dependency = Dependency {
                mark: Rc::downgrade(&reader.mark),
                built: Rc::downgrade(&reader.built),
                epoch: reader.epoch,
                whole: false,
                parts: Vec::new(),
            };
        }
        match part {
            None => {
                dependency.whole = true;
                dependency.parts.clear();
            }
            Some(part) if !dependency.whole => dependency.parts.push(part),
            Some(_) => {}
        }
    }

    /// The value changed: each reader it concerns is marked, and forgotten until its next
    /// build reads it again; the others are kept.
    fn changed(&self) {
        let readers = std::mem::take(&mut *self.shared.readers.borrow_mut());
        let mut marked = false;
        let mut kept = HashMap::new();
        {
            let value = self.shared.value.borrow();
            for (at, dependency) in readers {
                let Some(mark) = dependency.mark.upgrade() else {
                    continue;
                };
                // Built again since, without reading it: nothing to tell.
                if dependency
                    .built
                    .upgrade()
                    .is_none_or(|built| built.get() != dependency.epoch)
                {
                    continue;
                }
                if dependency.whole || dependency.parts.iter().any(|part| part(&value)) {
                    mark.set(true);
                    marked = true;
                } else {
                    kept.insert(at, dependency);
                }
            }
        }
        self.shared.readers.borrow_mut().extend(kept);
        if marked {
            crate::component::request_component_rebuild();
        }
        self.shared.listeners.tell_listeners();
    }
}

impl<T: Default + 'static> Default for Model<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> Listenable for Model<T> {
    /// Calls `listener` after every change. Listening asks for no rebuild: the readers are
    /// built again because they read it.
    fn add_listener(&self, listener: impl Fn() + 'static) -> Subscription {
        self.shared.listeners.add_listener(listener)
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Model<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Model")
            .field(&*self.shared.value.borrow())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        build_deferred, component, take_rebuild, BuildContext, Component, Container, Flex, Rebuild,
        Theme,
    };
    use frus_layout::Dimension;

    /// One frame of the shell's, less the drawing.
    fn frame(runtime: &Runtime, root: &dyn Widget) {
        runtime.states.begin_build();
        build_deferred(root, &Theme::default(), runtime);
        runtime.states.end_frame();
    }

    fn width(widget: &dyn Widget) -> Dimension {
        widget.style().width
    }

    #[derive(Default)]
    struct Cart {
        items: Vec<&'static str>,
        note: String,
    }

    /// How many times each part of one shop was built.
    #[derive(Default)]
    struct Builds {
        shop: Cell<u32>,
        lines: Cell<u32>,
        badge: Cell<u32>,
        button: Cell<u32>,
    }

    fn bump(count: &Cell<u32>) {
        count.set(count.get() + 1);
    }

    type Out = Rc<RefCell<Option<Model<Cart>>>>;

    /// A shop: it owns a cart and puts it in scope for its lines (which watch the cart), its
    /// badge (which selects the number of items) and its button (which only reads it).
    fn shop(builds: Rc<Builds>, out: Out) -> Component {
        component(move |cx| {
            bump(&builds.shop);
            let cart = cx.use_model(Cart::default);
            *out.borrow_mut() = Some(cart.clone());
            let (lines, badge, button) = (builds.clone(), builds.clone(), builds.clone());
            Box::new(Scoped::new(
                cart,
                Flex::column()
                    .child(component(move |cx| {
                        bump(&lines.lines);
                        let cart = cx.watch::<Cart>();
                        let shown = cart.with(|c| c.items.len() * 10 + c.note.len());
                        Box::new(Container::new().width(shown as f32).height(2.0))
                    }))
                    .child(component(move |cx| {
                        bump(&badge.badge);
                        let count = cx.select(|c: &Cart| c.items.len());
                        Box::new(Container::new().width(count as f32).height(2.0))
                    }))
                    .child(component(move |cx| {
                        bump(&button.button);
                        let _cart = cx.read::<Cart>();
                        Box::new(Container::new().width(1.0).height(2.0))
                    })),
            ))
        })
    }

    fn counts(builds: &Builds) -> [u32; 4] {
        [
            builds.shop.get(),
            builds.lines.get(),
            builds.badge.get(),
            builds.button.get(),
        ]
    }

    /// **Issue #222's proof**: two instances of a screen each put their own cart in scope, and
    /// a change to one builds that one's readers, and nothing else.
    #[test]
    fn two_instances_each_have_their_own_model_and_a_change_builds_only_its_readers() {
        let runtime = Runtime::default();
        let (a, b) = (Rc::new(Builds::default()), Rc::new(Builds::default()));
        let (cart_a, cart_b): (Out, Out) = (Rc::default(), Rc::default());
        let root = Flex::column()
            .child(shop(a.clone(), cart_a.clone()))
            .child(shop(b.clone(), cart_b.clone()));
        frame(&runtime, &root);
        let _ = take_rebuild();
        assert_eq!(counts(&a), [1, 1, 1, 1]);
        assert_eq!(counts(&b), [1, 1, 1, 1]);
        let (cart_a, cart_b) = (
            cart_a.borrow().clone().unwrap(),
            cart_b.borrow().clone().unwrap(),
        );
        assert!(!cart_a.ptr_eq(&cart_b), "each shop has a cart of its own");
        assert_eq!((cart_a.reader_count(), cart_b.reader_count()), (2, 2));

        cart_a.update(|c| c.items.push("tea"));
        assert_eq!(
            take_rebuild(),
            Rebuild::Components,
            "a model's change builds its readers, not the tree"
        );
        frame(&runtime, &root);
        assert_eq!(
            counts(&a),
            [1, 2, 2, 1],
            "the lines and the badge of the first shop are built again; the shop and its \
             button are not"
        );
        assert_eq!(counts(&b), [1, 1, 1, 1], "the other shop is not touched");
        let shop_a = &root.children()[0];
        assert_eq!(
            width(shop_a.children()[0].as_ref()),
            Dimension::Length(10.0)
        );
        assert_eq!(width(shop_a.children()[1].as_ref()), Dimension::Length(1.0));
        let shop_b = &root.children()[1];
        assert_eq!(width(shop_b.children()[0].as_ref()), Dimension::Length(0.0));

        cart_b.update(|c| c.items.extend(["bread", "jam"]));
        let _ = take_rebuild();
        frame(&runtime, &root);
        assert_eq!(counts(&a), [1, 2, 2, 1]);
        assert_eq!(counts(&b), [1, 2, 2, 1]);
        let shop_b = &root.children()[1];
        assert_eq!(
            width(shop_b.children()[0].as_ref()),
            Dimension::Length(20.0)
        );
        assert_eq!(
            cart_a.with(|c| c.items.len()),
            1,
            "the first cart is as it was"
        );
    }

    /// **A selector** builds its component only when the part it selects changes.
    #[test]
    fn a_selector_builds_only_when_its_part_changes() {
        let runtime = Runtime::default();
        let (builds, cart): (_, Out) = (Rc::new(Builds::default()), Rc::default());
        let root = shop(builds.clone(), cart.clone());
        frame(&runtime, &root);
        let cart = cart.borrow().clone().unwrap();

        cart.update(|c| c.note = "gift".into());
        let _ = take_rebuild();
        frame(&runtime, &root);
        assert_eq!(
            counts(&builds),
            [1, 2, 1, 1],
            "a new note: the lines show it, the badge counts items and is not built"
        );

        cart.update(|c| c.note.push('!'));
        let _ = take_rebuild();
        frame(&runtime, &root);
        assert_eq!(counts(&builds), [1, 3, 1, 1], "still not the badge");

        cart.update(|c| c.items.push("tea"));
        let _ = take_rebuild();
        frame(&runtime, &root);
        assert_eq!(
            counts(&builds),
            [1, 4, 2, 1],
            "one more item: the badge too"
        );
        assert_eq!(
            width(root.children()[1].as_ref()),
            Dimension::Length(1.0),
            "and it shows the new count"
        );
    }

    /// A change that concerns no reader builds nothing, and asks for no frame to.
    #[test]
    fn a_change_no_reader_cares_about_asks_for_nothing() {
        let runtime = Runtime::default();
        let model = Model::new(Cart::default());
        let built = Rc::new(Cell::new(0));
        let counted = built.clone();
        let root = Scoped::new(
            model.clone(),
            component(move |cx| {
                bump(&counted);
                let count = cx.select(|c: &Cart| c.items.len());
                Box::new(Container::new().width(count as f32))
            }),
        );
        frame(&runtime, &root);
        let _ = take_rebuild();
        model.update(|c| c.note = "x".into());
        assert_eq!(take_rebuild(), Rebuild::Nothing);
        frame(&runtime, &root);
        assert_eq!(built.get(), 1);
    }

    /// The nearest value wins; a component built directly by a `Scoped` finds it; and with
    /// no `Scoped` above, the one provided for the whole application is found.
    #[test]
    fn the_nearest_scope_wins_and_the_application_is_the_outermost() {
        let runtime = Runtime::default();
        let seen: Rc<RefCell<Vec<u32>>> = Rc::default();
        let reader = |seen: &Rc<RefCell<Vec<u32>>>| {
            let seen = seen.clone();
            component(move |cx: &BuildContext| {
                seen.borrow_mut().push(cx.watch::<u32>().get());
                Box::new(Container::new())
            })
        };
        runtime.states.provide(Rc::new(Model::new(1u32)));
        let (outer, inner) = (Model::new(2u32), Model::new(3u32));
        let root = Flex::column().child(reader(&seen)).child(Scoped::new(
            outer.clone(),
            Flex::column()
                .child(reader(&seen))
                .child(Scoped::new(inner.clone(), reader(&seen))),
        ));
        frame(&runtime, &root);
        assert_eq!(*seen.borrow(), [1, 2, 3]);

        seen.borrow_mut().clear();
        inner.set(4);
        let _ = take_rebuild();
        frame(&runtime, &root);
        assert_eq!(*seen.borrow(), [4], "the inner one's reader alone");

        seen.borrow_mut().clear();
        runtime
            .states
            .service::<Model<u32>>()
            .unwrap()
            .update(|n| *n = 5);
        let _ = take_rebuild();
        frame(&runtime, &root);
        assert_eq!(*seen.borrow(), [5], "the application's one's reader alone");
    }

    /// A component that read a model in one build and not in the next is not built by the
    /// model's later changes.
    #[test]
    fn a_component_that_stops_reading_stops_being_built() {
        let runtime = Runtime::default();
        let model = Model::new(0u32);
        let built = Rc::new(Cell::new(0));
        let flag: Rc<RefCell<Option<crate::UseState<bool>>>> = Rc::default();
        let (counted, out) = (built.clone(), flag.clone());
        let root = Scoped::new(
            model.clone(),
            component(move |cx| {
                bump(&counted);
                let reading = cx.use_state(|| true);
                *out.borrow_mut() = Some(reading.clone());
                if reading.get() {
                    let _ = cx.watch::<u32>();
                }
                Box::new(Container::new())
            }),
        );
        frame(&runtime, &root);
        flag.borrow().as_ref().unwrap().set(false);
        let _ = take_rebuild();
        frame(&runtime, &root);
        assert_eq!(built.get(), 2);

        model.set(1);
        assert_eq!(take_rebuild(), Rebuild::Nothing, "nobody reads it any more");
        frame(&runtime, &root);
        assert_eq!(built.get(), 2);
        assert_eq!(model.reader_count(), 0, "and it forgot the reader");
    }

    /// Watching the whole and selecting a part in the same build: any change builds it.
    #[test]
    fn watching_the_whole_outweighs_a_part() {
        let runtime = Runtime::default();
        let model = Model::new(Cart::default());
        let built = Rc::new(Cell::new(0));
        let counted = built.clone();
        let root = Scoped::new(
            model.clone(),
            component(move |cx| {
                bump(&counted);
                let _ = cx.select(|c: &Cart| c.items.len());
                let _ = cx.watch::<Cart>();
                let _ = cx.select(|c: &Cart| c.items.len());
                Box::new(Container::new())
            }),
        );
        frame(&runtime, &root);
        model.update(|c| c.note = "x".into());
        let _ = take_rebuild();
        frame(&runtime, &root);
        assert_eq!(built.get(), 2);
    }

    /// A listener hears every change, and a change asks for no rebuild of the tree.
    #[test]
    fn a_listener_hears_a_change_without_a_rebuild_of_the_tree() {
        let model = Model::new(0);
        let heard = Rc::new(Cell::new(0));
        let counted = heard.clone();
        let _listening = model.add_listener(move || bump(&counted));
        let _ = take_rebuild();
        model.set(1);
        model.set(1);
        model.update(|n| *n += 1);
        assert_eq!(heard.get(), 2, "the same value is not a change");
        assert_eq!(
            take_rebuild(),
            Rebuild::Nothing,
            "no reader: nothing to build"
        );
    }

    #[test]
    #[should_panic(expected = "no Model<u8> above this component")]
    fn reading_a_model_nobody_provided_says_how_to_provide_one() {
        let runtime = Runtime::default();
        frame(
            &runtime,
            &component(|cx| {
                let _ = cx.watch::<u8>();
                Box::new(Container::new())
            }),
        );
    }
}
