use telorgon::app::*;

/// Ask client-decorated Wayland apps for tiled styling while retaining floating placement.
pub const DECORATION_POLICY: DecorationPolicy = DecorationPolicy {
    tiled_client_decorations: true,
    ..DecorationPolicy::DEFAULT
};
