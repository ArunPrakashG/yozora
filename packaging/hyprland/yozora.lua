-- Yozora for Hyprland 0.55 and later (Lua config).
-- Copy into ~/.config/hypr/ and add `require("yozora")` to hyprland.lua, or paste the parts you want.

-- Start the bar and the notification server. Skip this if you use the systemd units
-- (`systemctl --user enable --now yozora-bar yozora-notify`). Stop mako or dunst first: only one
-- notification server can run.
hl.on("hyprland.start", function()
    hl.exec_cmd("yozora-bar")
    hl.exec_cmd("yozora-notify")
end)

-- Blur behind the bar, its popovers and toasts. The bar and yozora-notify add these at runtime too, but rules added at
-- runtime are lost when Hyprland reloads its config.
hl.layer_rule({ match = { namespace = "^yozora-" }, blur = true, blur_popups = true, ignore_alpha = 0.2, no_anim = true })

-- Settings opens as a floating window at its own size.
hl.window_rule({
    name = "yozora-settings",
    match = { class = "^(yozora-settings)$" },
    float = true,
    size = "1180 800",
    center = true,
})

-- Volume, mute and brightness keys. The bar shows its OSD for any change it sees, so plain
-- pactl is enough; there is no pipe to feed. Brightness has no change events, so its keys go
-- through the bar. Remove wob or swayosd if you used them.
hl.bind("XF86AudioRaiseVolume", hl.dsp.exec_cmd("pactl set-sink-volume @DEFAULT_SINK@ +5%"), { repeating = true, locked = true })
hl.bind("XF86AudioLowerVolume", hl.dsp.exec_cmd("pactl set-sink-volume @DEFAULT_SINK@ -5%"), { repeating = true, locked = true })
hl.bind("XF86AudioMute", hl.dsp.exec_cmd("pactl set-sink-mute @DEFAULT_SINK@ toggle"), { locked = true })
hl.bind("XF86AudioMicMute", hl.dsp.exec_cmd("pactl set-source-mute @DEFAULT_SOURCE@ toggle"), { locked = true })
hl.bind("XF86MonBrightnessUp", hl.dsp.exec_cmd("yozora-bar osd brightness +5%"), { repeating = true, locked = true })
hl.bind("XF86MonBrightnessDown", hl.dsp.exec_cmd("yozora-bar osd brightness -5%"), { repeating = true, locked = true })

-- The lock screen runs from hypridle: in ~/.config/hypr/hypridle.conf, set
-- `lock_cmd = yozora-lock`, `before_sleep_cmd = loginctl lock-session` and `inhibit_sleep = 3`.
-- Then this key locks too:
-- hl.bind("SUPER + L", hl.dsp.exec_cmd("loginctl lock-session"))

-- Optional keys for the bar's popovers.
-- hl.bind("SUPER + N", hl.dsp.exec_cmd("yozora-bar msg toggle notifications"))
-- hl.bind("SUPER + A", hl.dsp.exec_cmd("yozora-bar msg toggle control-center"))
