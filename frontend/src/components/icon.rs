use leptos::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconName {
    Brain,
    ChevronLeft,
    Settings,
    ChevronDown,
    Contrast,
    Error,
    LogoMark,
    Moon,
    Send,
    Spinner,
    Stop,
    Sun,
}

impl IconName {
    fn markup(self) -> &'static str {
        match self {
            Self::Brain => include_str!("../../assets/icons/brain.svg"),
            Self::ChevronLeft => include_str!("../../assets/icons/chevron-left.svg"),
            Self::Settings => include_str!("../../assets/icons/settings.svg"),
            Self::ChevronDown => include_str!("../../assets/icons/chevron-down.svg"),
            Self::Contrast => include_str!("../../assets/icons/contrast.svg"),
            Self::Error => include_str!("../../assets/icons/error.svg"),
            Self::LogoMark => include_str!("../../assets/icons/logo-mark.svg"),
            Self::Moon => include_str!("../../assets/icons/moon.svg"),
            Self::Send => include_str!("../../assets/icons/send.svg"),
            Self::Spinner => include_str!("../../assets/icons/spinner.svg"),
            Self::Stop => include_str!("../../assets/icons/stop.svg"),
            Self::Sun => include_str!("../../assets/icons/sun.svg"),
        }
    }

    fn modifier(self) -> &'static str {
        match self {
            Self::Spinner => "icon icon-spinner",
            Self::Send => "icon icon-directional",
            _ => "icon",
        }
    }
}

#[component]
pub fn Icon(name: IconName) -> impl IntoView {
    view! { <span class=name.modifier() aria-hidden="true" inner_html=name.markup()></span> }
}
