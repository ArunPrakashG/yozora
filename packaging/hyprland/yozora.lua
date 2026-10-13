-- Yozora for Hyprland 0.55 and later (Lua config).
-- Copy into ~/.config/hypr/ and add `require("yozora")` to hyprland.lua, or paste the parts you want.

-- Start the bar. Skip this if you use the systemd unit (`systemctl --user enable --now yozora-bar`).
hl.on("hyprland.start", function()
    hl.exec_cmd("yozora-bar")
end)

-- Blur behind the bar and its popovers. The bar adds these at runtime too, but rules added at
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

-- Optional keys for the bar's popovers.
-- hl.bind("SUPER + N", hl.dsp.exec_cmd("yozora-bar msg toggle notifications"))
-- hl.bind("SUPER + A", hl.dsp.exec_cmd("yozora-bar msg toggle control-center"))
