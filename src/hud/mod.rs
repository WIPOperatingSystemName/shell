//! Passive feedback for admitted system controls. The mixer and brightness service own writes.
mod model;
mod owner;
mod view;

pub(crate) use owner::SystemHuds;
pub(crate) use view::SystemHud;

#[cfg(test)]
mod tests;
