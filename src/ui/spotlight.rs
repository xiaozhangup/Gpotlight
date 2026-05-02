use crate::config::ConfigStore;
use crate::i18n::I18n;
use crate::plugin::{activate_result, SearchResult, SharedRegistry};
use crate::theme;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub struct SpotlightWindow {
    window: gtk::Window,
    entry: gtk::SearchEntry,
    panel: gtk::Box,
    results_view: gtk::Box,
    list: gtk::ListBox,
    scroll_indicator: gtk::DrawingArea,
    results: Rc<RefCell<Vec<SearchResult>>>,
    result_offset: Rc<RefCell<usize>>,
    selected_index: Rc<RefCell<usize>>,
    config: Rc<RefCell<ConfigStore>>,
    plugins: SharedRegistry,
}

impl SpotlightWindow {
    pub fn new(
        app: &impl IsA<gtk::Application>,
        i18n: Rc<I18n>,
        config: Rc<RefCell<ConfigStore>>,
        plugins: SharedRegistry,
    ) -> Self {
        install_css();

        let cfg = config.borrow().current().window.clone();
        let window = gtk::Window::builder()
            .title(i18n.t("app_name"))
            .default_width(cfg.host_width)
            .default_height(cfg.host_height)
            .decorated(false)
            .resizable(false)
            .build();
        let _ = app;
        window.add_css_class("transparent-host");
        theme::apply_to_window(&window);

        let host = gtk::Box::new(gtk::Orientation::Vertical, 0);
        host.set_halign(gtk::Align::Center);
        host.set_valign(gtk::Align::Start);
        host.set_margin_top(cfg.panel_offset_y);

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
        panel.set_width_request(cfg.panel_width);
        panel.add_css_class("spotlight-panel");

        let entry = gtk::SearchEntry::builder()
            .placeholder_text(i18n.t("placeholder"))
            .hexpand(true)
            .build();
        entry.add_css_class("spotlight-entry");

        let results = Rc::new(RefCell::new(Vec::new()));
        let result_offset = Rc::new(RefCell::new(0));
        let selected_index = Rc::new(RefCell::new(0));

        let list = gtk::ListBox::new();
        list.add_css_class("spotlight-results");
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_hexpand(true);
        list.set_visible(false);

        let scroll_indicator = gtk::DrawingArea::new();
        scroll_indicator.add_css_class("result-scroll-indicator");
        scroll_indicator.set_content_width(6);
        scroll_indicator.set_hexpand(false);
        scroll_indicator.set_valign(gtk::Align::Fill);
        scroll_indicator.set_visible(false);
        {
            let results = results.clone();
            let result_offset = result_offset.clone();
            let config = config.clone();
            scroll_indicator.set_draw_func(move |_, cr, width, height| {
                draw_scroll_indicator(
                    cr,
                    width,
                    height,
                    results.borrow().len(),
                    config.borrow().current().window.max_visible_results,
                    *result_offset.borrow(),
                );
            });
        }

        let results_view = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        results_view.add_css_class("results-view");
        results_view.set_hexpand(true);
        results_view.append(&list);
        results_view.append(&scroll_indicator);
        results_view.set_visible(false);

        panel.append(&entry);
        panel.append(&results_view);
        host.append(&panel);
        window.set_child(Some(&host));

        let this = Self {
            window,
            entry,
            panel,
            results_view,
            list,
            scroll_indicator,
            results,
            result_offset,
            selected_index,
            config,
            plugins,
        };
        this.connect_signals();
        this
    }

    pub fn prime(&self) {
        self.window.set_visible(false);
    }

    pub fn toggle(&self) {
        if self.window.is_visible() {
            self.window.set_visible(false);
        } else {
            self.present();
        }
    }

    pub fn present(&self) {
        self.entry.set_text("");
        self.clear_results();
        self.window.present();
        self.entry.grab_focus();
    }

    pub fn apply_window_config(&self) {
        let cfg = self.config.borrow().current().window.clone();
        self.window
            .set_default_size(cfg.host_width, cfg.host_height);
        if let Some(host) = self
            .window
            .child()
            .and_then(|child| child.downcast::<gtk::Box>().ok())
        {
            host.set_margin_top(cfg.panel_offset_y);
            if let Some(panel) = host
                .first_child()
                .and_then(|child| child.downcast::<gtk::Box>().ok())
            {
                panel.set_width_request(cfg.panel_width);
            }
        }
        render_results(
            &self.list,
            &self.results_view,
            &self.scroll_indicator,
            &self.results.borrow(),
            *self.result_offset.borrow(),
            *self.selected_index.borrow(),
            cfg.max_visible_results,
        );
    }

    fn connect_signals(&self) {
        let list = self.list.clone();
        let results_view = self.results_view.clone();
        let scroll_indicator = self.scroll_indicator.clone();
        let results = self.results.clone();
        let result_offset = self.result_offset.clone();
        let selected_index = self.selected_index.clone();
        let config = self.config.clone();
        let plugins = self.plugins.clone();
        self.entry.connect_search_changed(move |entry| {
            let query = entry.text().to_string();
            let found = plugins.borrow().search(&config.borrow(), &query);
            *result_offset.borrow_mut() = 0;
            *selected_index.borrow_mut() = 0;
            render_results(
                &list,
                &results_view,
                &scroll_indicator,
                &found,
                0,
                0,
                config.borrow().current().window.max_visible_results,
            );
            *results.borrow_mut() = found;
        });

        let results = self.results.clone();
        let selected_index = self.selected_index.clone();
        let window = self.window.clone();
        self.entry.connect_activate(move |_| {
            if let Some(result) = results.borrow().get(*selected_index.borrow()) {
                activate_result(result, window.upcast_ref());
            }
        });

        let results = self.results.clone();
        let result_offset = self.result_offset.clone();
        let selected_index = self.selected_index.clone();
        let window = self.window.clone();
        self.list.connect_row_activated(move |_, row| {
            let index = *result_offset.borrow() + row.index() as usize;
            *selected_index.borrow_mut() = index;
            if let Some(result) = results.borrow().get(index) {
                activate_result(result, window.upcast_ref());
            }
        });

        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let list = self.list.clone();
        let results_view = self.results_view.clone();
        let scroll_indicator = self.scroll_indicator.clone();
        let results = self.results.clone();
        let result_offset = self.result_offset.clone();
        let selected_index = self.selected_index.clone();
        let config = self.config.clone();
        scroll.connect_scroll(move |_, _, dy| {
            let results_ref = results.borrow();
            if results_ref.is_empty() {
                return glib::Propagation::Proceed;
            }

            let current_selected = *selected_index.borrow();
            let next_selected = if dy > 0.0 {
                (current_selected + 1).min(results_ref.len() - 1)
            } else if dy < 0.0 {
                current_selected.saturating_sub(1)
            } else {
                current_selected
            };

            if next_selected != current_selected {
                let visible_count = visible_result_count(
                    results_ref.len(),
                    config.borrow().current().window.max_visible_results,
                );
                let current_offset = *result_offset.borrow();
                let next_offset =
                    offset_for_selection(current_offset, next_selected, visible_count);

                *selected_index.borrow_mut() = next_selected;
                *result_offset.borrow_mut() = next_offset;
                render_results(
                    &list,
                    &results_view,
                    &scroll_indicator,
                    &results_ref,
                    next_offset,
                    next_selected,
                    config.borrow().current().window.max_visible_results,
                );
            }
            glib::Propagation::Stop
        });
        self.list.add_controller(scroll);

        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        let window = self.window.clone();
        let list = self.list.clone();
        let results_view = self.results_view.clone();
        let scroll_indicator = self.scroll_indicator.clone();
        let results = self.results.clone();
        let result_offset = self.result_offset.clone();
        let selected_index = self.selected_index.clone();
        let config = self.config.clone();
        key.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                window.set_visible(false);
                return glib::Propagation::Stop;
            }

            if key == gtk::gdk::Key::Down || key == gtk::gdk::Key::Up {
                let results_ref = results.borrow();
                if results_ref.is_empty() {
                    return glib::Propagation::Stop;
                }

                let current_selected = *selected_index.borrow();
                let next_selected = if key == gtk::gdk::Key::Down {
                    (current_selected + 1).min(results_ref.len() - 1)
                } else {
                    current_selected.saturating_sub(1)
                };
                let visible_count = visible_result_count(
                    results_ref.len(),
                    config.borrow().current().window.max_visible_results,
                );
                let current_offset = *result_offset.borrow();
                let next_offset =
                    offset_for_selection(current_offset, next_selected, visible_count);

                *selected_index.borrow_mut() = next_selected;
                *result_offset.borrow_mut() = next_offset;
                render_results(
                    &list,
                    &results_view,
                    &scroll_indicator,
                    &results_ref,
                    next_offset,
                    next_selected,
                    config.borrow().current().window.max_visible_results,
                );
                return glib::Propagation::Stop;
            }

            glib::Propagation::Proceed
        });
        self.window.add_controller(key);

        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let panel = self.panel.clone();
        let window = self.window.clone();
        click.connect_pressed(move |gesture, _, x, y| {
            let Some(widget) = gesture.widget() else {
                return;
            };
            let Some(panel_bounds) = panel.compute_bounds(&widget) else {
                return;
            };
            let inside_panel = x >= panel_bounds.x() as f64
                && x <= (panel_bounds.x() + panel_bounds.width()) as f64
                && y >= panel_bounds.y() as f64
                && y <= (panel_bounds.y() + panel_bounds.height()) as f64;

            if !inside_panel {
                window.set_visible(false);
                gesture.set_state(gtk::EventSequenceState::Claimed);
            }
        });
        self.window.add_controller(click);

        self.window.connect_is_active_notify(|window| {
            if !window.is_active() {
                window.set_visible(false);
            }
        });
    }

    fn clear_results(&self) {
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        self.list.set_visible(false);
        self.results_view.set_visible(false);
        self.scroll_indicator.set_visible(false);
        self.results.borrow_mut().clear();
        *self.result_offset.borrow_mut() = 0;
        *self.selected_index.borrow_mut() = 0;
    }
}

fn render_results(
    list: &gtk::ListBox,
    results_view: &gtk::Box,
    scroll_indicator: &gtk::DrawingArea,
    results: &[SearchResult],
    offset: usize,
    selected_index: usize,
    max_visible_results: i32,
) {
    while let Some(row) = list.first_child() {
        list.remove(&row);
    }

    let visible_count = visible_result_count(results.len(), max_visible_results);
    for result in results.iter().skip(offset).take(visible_count) {
        list.append(&result_row(result));
    }
    let has_results = visible_count > 0;
    list.set_visible(has_results);
    results_view.set_visible(has_results);
    update_selection(list, offset, selected_index);
    update_scroll_indicator(scroll_indicator, results.len(), visible_count);
}

fn visible_result_count(result_count: usize, max_visible_results: i32) -> usize {
    result_count.min(max_visible_results.clamp(1, 20) as usize)
}

fn offset_for_selection(
    current_offset: usize,
    selected_index: usize,
    visible_count: usize,
) -> usize {
    if selected_index < current_offset {
        selected_index
    } else if selected_index >= current_offset + visible_count {
        selected_index + 1 - visible_count
    } else {
        current_offset
    }
}

fn update_selection(list: &gtk::ListBox, offset: usize, selected_index: usize) {
    let visible_index = selected_index.saturating_sub(offset) as i32;
    if let Some(row) = list.row_at_index(visible_index) {
        list.select_row(Some(&row));
    }
}

fn update_scroll_indicator(indicator: &gtk::DrawingArea, total: usize, visible: usize) {
    if total <= visible || visible == 0 {
        indicator.set_visible(false);
        return;
    }

    indicator.set_visible(true);
    indicator.queue_draw();
}

fn draw_scroll_indicator(
    cr: &gtk::cairo::Context,
    width: i32,
    height: i32,
    total: usize,
    max_visible_results: i32,
    offset: usize,
) {
    let visible = visible_result_count(total, max_visible_results);
    if total <= visible || visible == 0 || width <= 0 || height <= 0 {
        return;
    }

    let total = total as f64;
    let visible = visible as f64;
    let offset = offset as f64;
    let width = width as f64;
    let track_height = height as f64;
    let thumb_height = (visible / total * track_height).clamp(18.0, track_height);
    let max_offset = (total - visible).max(1.0);
    let thumb_top = ((track_height - thumb_height) * (offset / max_offset)).round();
    let track_width = 3.0_f64.min(width);
    let x = ((width - track_width) / 2.0).round();
    let radius = track_width / 2.0;
    let is_dark = adw::StyleManager::default().is_dark();

    if is_dark {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
    } else {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.06);
    }
    rounded_rect(cr, x, 0.0, track_width, track_height, radius);
    let _ = cr.fill();

    if is_dark {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.42);
    } else {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.34);
    }
    rounded_rect(cr, x, thumb_top, track_width, thumb_height, radius);
    let _ = cr.fill();
}

fn rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width / 2.0).min(height / 2.0);
    cr.new_sub_path();
    cr.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    cr.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    cr.close_path();
}

fn result_row(result: &SearchResult) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_hexpand(true);
    row.set_height_request(50);
    let outer = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    outer.add_css_class("result-row");
    outer.set_hexpand(true);
    outer.set_height_request(44);

    let image =
        gtk::Image::from_icon_name(result.icon.as_deref().unwrap_or("system-search-symbolic"));
    image.set_pixel_size(22);

    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_hexpand(true);
    let title = gtk::Label::new(Some(&result.title));
    title.set_halign(gtk::Align::Start);
    title.set_hexpand(true);
    title.set_single_line_mode(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("result-title");

    let subtitle_text = if result.subtitle.is_empty() {
        " "
    } else {
        &result.subtitle
    };
    let subtitle = gtk::Label::new(Some(subtitle_text));
    subtitle.set_halign(gtk::Align::Start);
    subtitle.set_hexpand(true);
    subtitle.set_single_line_mode(true);
    subtitle.add_css_class("result-subtitle");
    subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);

    labels.append(&title);
    labels.append(&subtitle);
    outer.append(&image);
    outer.append(&labels);
    row.set_child(Some(&outer));
    row
}

fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        r#"
        window.transparent-host {
            background: transparent;
        }
        .spotlight-panel {
            border: 1px solid alpha(@theme_fg_color, 0.16);
            border-radius: 18px;
            padding: 10px 10px 10px 10px;
            box-shadow: 0 18px 48px alpha(black, 0.34);
        }
        window.system-light .spotlight-panel {
            background: rgba(250, 250, 250, 0.94);
            color: rgb(36, 36, 36);
        }
        window.system-dark .spotlight-panel {
            background: rgba(34, 34, 36, 0.94);
            color: rgb(238, 238, 238);
        }
        .spotlight-entry {
            min-height: 48px;
            font-size: 20px;
            border-radius: 12px;
            margin: 0;
        }
        .spotlight-results {
            background: transparent;
            margin-top: 0;
        }
        .results-view {
            margin: 0;
        }
        .result-scroll-indicator {
            margin-top: 6px;
            min-width: 6px;
        }
        .spotlight-results row {
            border-radius: 10px;
            margin-top: 4px;
            transition: background-color 80ms ease-out;
        }
        .spotlight-results row:first-child {
            margin-top: 6px;
        }
        .spotlight-results row:selected,
        .spotlight-results row:selected:hover,
        .spotlight-results row:selected:focus {
            background: alpha(@accent_bg_color, 0.20);
            border-radius: 10px;
        }
        .spotlight-results row:selected .result-row {
            border-radius: 10px;
        }
        .result-row {
            padding: 4px 8px;
            border-radius: 10px;
            min-height: 44px;
            transition: background-color 80ms ease-out;
        }
        .result-title {
            font-weight: 600;
        }
        .result-subtitle {
            opacity: 0.72;
            font-size: 12px;
        }
        "#,
    );

    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
