-- Minimal Hyprland (0.57+, Lua config) for the wear-pointer desktop harness.
hl.monitor({
    output   = "",
    mode     = "1920x1080@60",
    position = "0x0",
    scale    = 1,
})

hl.config({
    debug = {
        disable_logs       = false,
        enable_stdout_logs = true,
    },
    misc = {
        disable_hyprland_logo    = true,
        disable_splash_rendering = true,
    },
    animations = {
        enabled = false,
    },
    general = {
        gaps_in     = 0,
        gaps_out    = 0,
        border_size = 0,
    },
    cursor = {
        -- virtio-vga in the harness VM has no hardware cursor plane.
        no_hardware_cursors = 1,
    },
})
