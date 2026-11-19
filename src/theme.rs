//! Colour themes: window colours plus the status palette.

use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    Light,
    Dark,
    Mint,
    Ocean,
    Sakura,
    Contrast,
}

impl Theme {
    pub const ALL: [Theme; 6] = [Theme::Light, Theme::Dark, Theme::Mint, Theme::Ocean, Theme::Sakura, Theme::Contrast];

    /// Text key in i18n.
    pub fn name_key(self) -> &'static str {
        match self {
            Theme::Light => "theme_light",
            Theme::Dark => "theme_dark",
            Theme::Mint => "theme_mint",
            Theme::Ocean => "theme_ocean",
            Theme::Sakura => "theme_sakura",
            Theme::Contrast => "theme_contrast",
        }
    }

    pub fn palette(self) -> Palette {
        let rgb = Color32::from_rgb;
        let base_light = Palette {
            dark: false,
            bg: Color32::from_gray(246),
            text: Color32::from_gray(30),
            muted: Color32::from_gray(95),
            value_bg: Color32::WHITE,
            stroke: Color32::from_gray(190),
            tab_bg: Color32::from_gray(240),
            tab_sel_bg: rgb(220, 232, 250),
            accent: rgb(90, 140, 220),
            stripe: Color32::from_gray(242),
            banner_bg: rgb(255, 246, 210),
            banner_stroke: rgb(220, 180, 60),
            banner_text: rgb(70, 55, 10),
            error: rgb(170, 60, 20),
            good: rgb(45, 120, 215),
            caution: rgb(240, 190, 30),
            bad: rgb(215, 50, 45),
            unknown: rgb(112, 112, 118),
        };
        let base_dark = Palette {
            dark: true,
            bg: rgb(32, 34, 38),
            text: Color32::from_gray(225),
            muted: Color32::from_gray(160),
            value_bg: rgb(22, 23, 26),
            stroke: Color32::from_gray(75),
            tab_bg: rgb(42, 44, 50),
            tab_sel_bg: rgb(40, 62, 98),
            accent: rgb(90, 150, 235),
            stripe: rgb(38, 40, 45),
            banner_bg: rgb(70, 58, 20),
            banner_stroke: rgb(170, 135, 40),
            banner_text: rgb(250, 230, 170),
            error: rgb(255, 140, 100),
            good: rgb(70, 145, 235),
            caution: rgb(235, 185, 40),
            bad: rgb(225, 65, 60),
            unknown: rgb(105, 105, 112),
        };
        match self {
            Theme::Light => base_light,
            Theme::Dark => base_dark,
            Theme::Mint => Palette {
                bg: rgb(238, 247, 241),
                tab_bg: rgb(228, 241, 233),
                tab_sel_bg: rgb(204, 236, 214),
                accent: rgb(40, 150, 90),
                stripe: rgb(234, 245, 238),
                stroke: rgb(170, 200, 180),
                good: rgb(46, 170, 87),
                ..base_light
            },
            Theme::Ocean => Palette {
                bg: rgb(16, 30, 52),
                value_bg: rgb(10, 20, 38),
                tab_bg: rgb(22, 42, 70),
                tab_sel_bg: rgb(20, 80, 120),
                accent: rgb(60, 200, 230),
                stripe: rgb(20, 36, 60),
                stroke: rgb(50, 80, 115),
                text: rgb(215, 232, 245),
                muted: rgb(140, 170, 200),
                good: rgb(30, 160, 225),
                ..base_dark
            },
            Theme::Sakura => Palette {
                bg: rgb(252, 242, 246),
                tab_bg: rgb(248, 232, 239),
                tab_sel_bg: rgb(250, 214, 228),
                accent: rgb(215, 95, 145),
                stripe: rgb(250, 238, 243),
                stroke: rgb(220, 185, 200),
                good: rgb(220, 105, 155),
                ..base_light
            },
            Theme::Contrast => Palette {
                dark: true,
                bg: Color32::BLACK,
                text: Color32::WHITE,
                muted: Color32::from_gray(220),
                value_bg: Color32::BLACK,
                stroke: Color32::WHITE,
                tab_bg: Color32::BLACK,
                tab_sel_bg: rgb(0, 60, 120),
                accent: rgb(0, 220, 255),
                stripe: Color32::from_gray(25),
                banner_bg: Color32::BLACK,
                banner_stroke: rgb(255, 230, 0),
                banner_text: rgb(255, 230, 0),
                error: rgb(255, 120, 80),
                good: rgb(0, 140, 255),
                caution: rgb(255, 220, 0),
                bad: rgb(255, 40, 40),
                unknown: Color32::from_gray(120),
            },
        }
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub value_bg: Color32,
    pub stroke: Color32,
    pub tab_bg: Color32,
    pub tab_sel_bg: Color32,
    pub accent: Color32,
    pub stripe: Color32,
    pub banner_bg: Color32,
    pub banner_stroke: Color32,
    pub banner_text: Color32,
    pub error: Color32,
    pub good: Color32,
    pub caution: Color32,
    pub bad: Color32,
    pub unknown: Color32,
}

impl Palette {
    pub fn visuals(&self) -> egui::Visuals {
        let mut v = if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = self.bg;
        v.window_fill = self.bg;
        v.extreme_bg_color = self.value_bg;
        v.faint_bg_color = self.stripe;
        v.override_text_color = Some(self.text);
        v.selection.bg_fill = self.accent.gamma_multiply(0.6);
        v.hyperlink_color = self.accent;
        v.widgets.noninteractive.bg_stroke.color = self.stroke;
        v.widgets.inactive.weak_bg_fill = self.tab_bg;
        v.widgets.inactive.bg_fill = self.tab_bg;
        v.widgets.hovered.weak_bg_fill = self.tab_sel_bg;
        v.widgets.hovered.bg_fill = self.tab_sel_bg;
        v.widgets.active.weak_bg_fill = self.accent.gamma_multiply(0.7);
        v
    }
}
