use leptos::{logging, prelude::*};

use crate::components::icon::{Icon, IconName};

const STORAGE_KEY: &str = "barry.theme";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Theme {
    System,
    Light,
    Dark,
    Contrast,
}

impl Theme {
    const ALL: [Self; 4] = [Self::System, Self::Light, Self::Dark, Self::Contrast];

    fn value(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Contrast => "contrast",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::Contrast => "High contrast",
        }
    }

    fn icon(self) -> Option<IconName> {
        match self {
            Self::System => None,
            Self::Light => Some(IconName::Sun),
            Self::Dark => Some(IconName::Moon),
            Self::Contrast => Some(IconName::Contrast),
        }
    }

    fn from_value(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.value() == value)
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    window().local_storage().ok().flatten()
}

fn stored_theme() -> Theme {
    let stored = local_storage().and_then(|storage| storage.get_item(STORAGE_KEY).ok().flatten());
    stored
        .as_deref()
        .and_then(Theme::from_value)
        .unwrap_or(Theme::System)
}

fn apply(theme: Theme) {
    let Some(root) = document().document_element() else {
        logging::error!("the document has no root element");
        return;
    };
    let applied = match theme {
        Theme::System => root.remove_attribute("data-theme"),
        other => root.set_attribute("data-theme", other.value()),
    };
    if let Err(error) = applied {
        logging::error!("could not apply the theme: {error:?}");
    }
    let stored = local_storage().map(|storage| storage.set_item(STORAGE_KEY, theme.value()));
    if let Some(Err(error)) = stored {
        logging::warn!("the theme choice could not be saved: {error:?}");
    }
}

#[component]
pub fn ThemePicker() -> impl IntoView {
    let selected = RwSignal::new(stored_theme());
    Effect::new(move || apply(selected.get()));
    view! {
        <fieldset class="theme-picker">
            <legend class="visually-hidden">"Theme"</legend>
            {Theme::ALL
                .into_iter()
                .map(|theme| {
                    view! {
                        <label class="theme-option">
                            <input
                                type="radio"
                                name="theme"
                                value=theme.value()
                                prop:checked=move || selected.get() == theme
                                on:change=move |_| selected.set(theme)
                            />
                            {theme.icon().map(|name| view! { <Icon name=name /> })}
                            <span>{theme.label()}</span>
                        </label>
                    }
                })
                .collect_view()}
        </fieldset>
    }
}
