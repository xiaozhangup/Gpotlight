use crate::config::ConfigStore;
use crate::i18n::I18n;
use crate::plugin::{
    activate_action, activate_result, SearchResult, SearchResultButton, SharedRegistry,
};
use crate::theme;
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

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
    refresh_tickets: Rc<RefCell<HashMap<String, u64>>>,
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
        window.set_application(Some(app.as_ref()));
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
        let refresh_tickets = Rc::new(RefCell::new(HashMap::new()));

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
            refresh_tickets,
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
            self.refresh_tickets.borrow_mut().clear();
        } else {
            self.present();
        }
    }

    pub fn present(&self) {
        self.refresh_tickets.borrow_mut().clear();
        self.entry.set_text("");
        // Present the window immediately so it appears without delay, then load
        // search results on the next event-loop iteration.
        self.window.present();
        self.entry.grab_focus();
        let spotlight = self.clone_handles();
        glib::idle_add_local_once(move || {
            spotlight.refresh_results("");
        });
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
            cfg.panel_width,
            &self.window,
            &self.config,
            self,
        );
    }

    fn connect_signals(&self) {
        let spotlight = self.clone_handles();
        self.entry.connect_search_changed(move |entry| {
            let query = entry.text().to_string();
            spotlight.refresh_results(&query);
        });

        let results = self.results.clone();
        let selected_index = self.selected_index.clone();
        let window = self.window.clone();
        let config = self.config.clone();
        self.entry.connect_activate(move |_| {
            if let Some(result) = results.borrow().get(*selected_index.borrow()) {
                activate_result_with_usage(result, window.upcast_ref(), &config);
            }
        });

        let results = self.results.clone();
        let result_offset = self.result_offset.clone();
        let selected_index = self.selected_index.clone();
        let window = self.window.clone();
        let config = self.config.clone();
        self.list.connect_row_activated(move |_, row| {
            let index = *result_offset.borrow() + row.index() as usize;
            *selected_index.borrow_mut() = index;
            if let Some(result) = results.borrow().get(index) {
                activate_result_with_usage(result, window.upcast_ref(), &config);
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
        let window = self.window.clone();
        let spotlight = self.clone_handles();
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
                if next_offset == current_offset {
                    update_selection(&list, current_offset, next_selected);
                } else {
                    render_results(
                        &list,
                        &results_view,
                        &scroll_indicator,
                        &results_ref,
                        next_offset,
                        next_selected,
                        config.borrow().current().window.max_visible_results,
                        config.borrow().current().window.panel_width,
                        &window,
                        &config,
                        &spotlight,
                    );
                }
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
        let spotlight = self.clone_handles();
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
                if next_offset == current_offset {
                    update_selection(&list, current_offset, next_selected);
                } else {
                    render_results(
                        &list,
                        &results_view,
                        &scroll_indicator,
                        &results_ref,
                        next_offset,
                        next_selected,
                        config.borrow().current().window.max_visible_results,
                        config.borrow().current().window.panel_width,
                        &window,
                        &config,
                        &spotlight,
                    );
                }
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

    fn refresh_results(&self, query: &str) {
        let found = self.plugins.borrow().search(&self.config.borrow(), query);
        *self.result_offset.borrow_mut() = 0;
        *self.selected_index.borrow_mut() = 0;
        render_results(
            &self.list,
            &self.results_view,
            &self.scroll_indicator,
            &found,
            0,
            0,
            self.config.borrow().current().window.max_visible_results,
            self.config.borrow().current().window.panel_width,
            &self.window,
            &self.config,
            self,
        );
        *self.results.borrow_mut() = found;
    }

    fn refresh_result_after(&self, result: &SearchResult, delay: Duration, replace_pending: bool) {
        let Some(plugin_id) = result.source_plugin_id.clone() else {
            return;
        };
        let Some(refresh_key) = result.refresh_key.clone() else {
            return;
        };
        let ticket_key = format!("{plugin_id}:{refresh_key}");
        let ticket = {
            let mut tickets = self.refresh_tickets.borrow_mut();
            if !replace_pending && tickets.contains_key(&ticket_key) {
                return;
            }
            let ticket = tickets.entry(ticket_key.clone()).or_insert(0);
            *ticket = ticket.saturating_add(1);
            *ticket
        };

        let spotlight = self.clone_handles();
        glib::timeout_add_local_once(delay, move || {
            if !spotlight.window.is_visible() {
                if spotlight.refresh_tickets.borrow().get(&ticket_key) == Some(&ticket) {
                    spotlight.refresh_tickets.borrow_mut().remove(&ticket_key);
                }
                return;
            }
            if spotlight.refresh_tickets.borrow().get(&ticket_key) != Some(&ticket) {
                return;
            }
            spotlight.refresh_tickets.borrow_mut().remove(&ticket_key);
            spotlight.refresh_result(&plugin_id, &refresh_key);
        });
    }

    fn refresh_result(&self, plugin_id: &str, refresh_key: &str) {
        let query = self.entry.text().to_string();
        let plugin_query = {
            let config = self.config.borrow();
            config
                .plugin_query(plugin_id, &query)
                .map(str::to_string)
                .or_else(|| query.trim().is_empty().then(String::new))
        };
        let Some(plugin_query) = plugin_query else {
            return;
        };

        let replacements =
            self.plugins
                .borrow()
                .search_plugin(&self.config.borrow(), plugin_id, &plugin_query);
        let Some(replacement) = replacements
            .into_iter()
            .find(|result| result.refresh_key.as_deref() == Some(refresh_key))
        else {
            return;
        };

        let mut results = self.results.borrow_mut();
        let Some(index) = results.iter().position(|result| {
            result.source_plugin_id.as_deref() == Some(plugin_id)
                && result.refresh_key.as_deref() == Some(refresh_key)
        }) else {
            return;
        };
        results[index] = replacement;
        let result = results[index].clone();
        drop(results);

        let offset = *self.result_offset.borrow();
        let visible_count = visible_result_count(
            self.results.borrow().len(),
            self.config.borrow().current().window.max_visible_results,
        );
        if index < offset || index >= offset + visible_count {
            return;
        }
        let Some(row) = self.list.row_at_index((index - offset) as i32) else {
            return;
        };
        let max_text_width_chars = result_text_max_width_chars(
            self.config.borrow().current().window.panel_width,
            result.buttons.len(),
        );
        if !update_result_row_content(&row, &result, max_text_width_chars) {
            row.set_child(Some(&result_row_content(
                &result,
                max_text_width_chars,
                &self.window,
                &self.config,
                self,
            )));
        }
        if let Some(interval) = result.refresh_interval_ms {
            self.refresh_result_after(&result, Duration::from_millis(interval), false);
        }
    }

    fn clone_handles(&self) -> Self {
        Self {
            window: self.window.clone(),
            entry: self.entry.clone(),
            panel: self.panel.clone(),
            results_view: self.results_view.clone(),
            list: self.list.clone(),
            scroll_indicator: self.scroll_indicator.clone(),
            results: self.results.clone(),
            result_offset: self.result_offset.clone(),
            selected_index: self.selected_index.clone(),
            refresh_tickets: self.refresh_tickets.clone(),
            config: self.config.clone(),
            plugins: self.plugins.clone(),
        }
    }
}

fn activate_result_with_usage(
    result: &SearchResult,
    window: &gtk::Window,
    config: &Rc<RefCell<ConfigStore>>,
) {
    let key = result.usage_key();
    if let Err(err) = config.borrow_mut().record_usage(&key) {
        tracing::warn!(error = ?err, usage_key = key, "failed to record result usage");
    }
    activate_result(result, window);
}

fn activate_button_with_usage(
    result: &SearchResult,
    button: &SearchResultButton,
    window: &gtk::Window,
    config: &Rc<RefCell<ConfigStore>>,
) {
    let key = button.usage_key(result);
    if let Err(err) = config.borrow_mut().record_usage(&key) {
        tracing::warn!(error = ?err, usage_key = key, "failed to record result button usage");
    }
    activate_action(&button.action, window, button.close_on_activate);
}

fn render_results(
    list: &gtk::ListBox,
    results_view: &gtk::Box,
    scroll_indicator: &gtk::DrawingArea,
    results: &[SearchResult],
    offset: usize,
    selected_index: usize,
    max_visible_results: i32,
    panel_width: i32,
    window: &gtk::Window,
    config: &Rc<RefCell<ConfigStore>>,
    spotlight: &SpotlightWindow,
) {
    while let Some(row) = list.first_child() {
        list.remove(&row);
    }

    let visible_count = visible_result_count(results.len(), max_visible_results);
    for result in results.iter().skip(offset).take(visible_count) {
        let max_text_width_chars = result_text_max_width_chars(panel_width, result.buttons.len());
        list.append(&result_row(
            result,
            max_text_width_chars,
            window,
            config,
            spotlight,
        ));
    }
    let has_results = visible_count > 0;
    list.set_visible(has_results);
    results_view.set_visible(has_results);
    update_selection(list, offset, selected_index);
    update_scroll_indicator(scroll_indicator, results.len(), visible_count);

    for result in results.iter().skip(offset).take(visible_count) {
        if let Some(interval) = result.refresh_interval_ms {
            spotlight.refresh_result_after(result, Duration::from_millis(interval), false);
        }
    }
}

fn result_text_max_width_chars(panel_width: i32, button_count: usize) -> i32 {
    let button_width = (button_count as i32 * 32).min(160);
    ((panel_width - 120 - button_width) / 8).clamp(8, 96)
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

fn result_row(
    result: &SearchResult,
    max_text_width_chars: i32,
    window: &gtk::Window,
    config: &Rc<RefCell<ConfigStore>>,
    spotlight: &SpotlightWindow,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_hexpand(true);
    row.set_height_request(58);
    row.set_child(Some(&result_row_content(
        result,
        max_text_width_chars,
        window,
        config,
        spotlight,
    )));
    row
}

fn result_row_content(
    result: &SearchResult,
    max_text_width_chars: i32,
    window: &gtk::Window,
    config: &Rc<RefCell<ConfigStore>>,
    spotlight: &SpotlightWindow,
) -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    outer.add_css_class("result-row");
    outer.set_hexpand(true);
    outer.set_height_request(52);

    let image = result_image(result.icon.as_deref());
    image.set_pixel_size(32);
    image.set_widget_name("result-icon");

    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_hexpand(true);
    labels.set_halign(gtk::Align::Fill);
    labels.set_width_request(1);
    let title = gtk::Label::new(Some(&result.title));
    title.set_halign(gtk::Align::Start);
    title.set_hexpand(true);
    title.set_width_chars(1);
    title.set_max_width_chars(max_text_width_chars);
    title.set_single_line_mode(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("result-title");
    title.set_widget_name("result-title");

    let subtitle_text = if result.subtitle.is_empty() {
        " "
    } else {
        &result.subtitle
    };
    let subtitle = gtk::Label::new(Some(subtitle_text));
    subtitle.set_halign(gtk::Align::Start);
    subtitle.set_hexpand(true);
    subtitle.set_width_chars(1);
    subtitle.set_max_width_chars(max_text_width_chars);
    subtitle.set_single_line_mode(true);
    subtitle.add_css_class("result-subtitle");
    subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
    subtitle.set_widget_name("result-subtitle");

    labels.append(&title);
    labels.append(&subtitle);
    outer.append(&image);
    outer.append(&labels);

    if !result.buttons.is_empty() {
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        buttons.add_css_class("result-buttons");
        buttons.set_hexpand(false);
        buttons.set_halign(gtk::Align::End);
        buttons.set_valign(gtk::Align::Center);
        for button in &result.buttons {
            let action_button = result_action_button(button);
            let result = result.clone();
            let button = button.clone();
            let window = window.clone();
            let config = config.clone();
            let spotlight = spotlight.clone_handles();
            action_button.connect_clicked(move |_| {
                activate_button_with_usage(&result, &button, window.upcast_ref(), &config);
                if let Some(refresh_after_ms) = button.refresh_after_ms {
                    spotlight.refresh_result_after(
                        &result,
                        Duration::from_millis(refresh_after_ms),
                        true,
                    );
                }
            });
            buttons.append(&action_button);
        }
        outer.append(&buttons);
    }

    outer
}

fn result_action_button(button: &SearchResultButton) -> gtk::Button {
    let action_button = gtk::Button::new();
    action_button.add_css_class("flat");
    action_button.add_css_class("result-action-button");
    action_button.set_tooltip_text(Some(&button.title));
    action_button.set_valign(gtk::Align::Center);
    action_button.set_focus_on_click(false);

    if let Some(icon) = button.icon.as_deref().filter(|icon| !icon.is_empty()) {
        let image = result_image(Some(icon));
        image.set_pixel_size(16);
        action_button.set_child(Some(&image));
    } else {
        let image = gtk::Image::from_icon_name("view-more-symbolic");
        image.set_pixel_size(16);
        action_button.set_child(Some(&image));
    }

    action_button
}

fn update_result_row_content(
    row: &gtk::ListBoxRow,
    result: &SearchResult,
    max_text_width_chars: i32,
) -> bool {
    let Some(outer) = row
        .child()
        .and_then(|child| child.downcast::<gtk::Box>().ok())
    else {
        return false;
    };
    let buttons_match = outer
        .last_child()
        .and_then(|child| child.downcast::<gtk::Box>().ok())
        .map(|buttons| buttons.has_css_class("result-buttons"))
        .unwrap_or(false)
        == !result.buttons.is_empty();
    if !buttons_match {
        return false;
    }
    if let Some(buttons) = outer
        .last_child()
        .and_then(|child| child.downcast::<gtk::Box>().ok())
        .filter(|buttons| buttons.has_css_class("result-buttons"))
    {
        if !update_result_buttons(&buttons, &result.buttons) {
            return false;
        }
    }

    if let Some(image) = find_named_child::<gtk::Image>(&outer, "result-icon") {
        set_image_icon(&image, result.icon.as_deref(), 32);
    }
    if let Some(title) = find_named_child::<gtk::Label>(&outer, "result-title") {
        title.set_text(&result.title);
        title.set_max_width_chars(max_text_width_chars);
    }
    if let Some(subtitle) = find_named_child::<gtk::Label>(&outer, "result-subtitle") {
        let subtitle_text = if result.subtitle.is_empty() {
            " "
        } else {
            &result.subtitle
        };
        subtitle.set_text(subtitle_text);
        subtitle.set_max_width_chars(max_text_width_chars);
    }
    true
}

fn update_result_buttons(buttons: &gtk::Box, result_buttons: &[SearchResultButton]) -> bool {
    let mut child = buttons.first_child();
    for result_button in result_buttons {
        let Some(current) = child else {
            return false;
        };
        let Ok(button) = current.clone().downcast::<gtk::Button>() else {
            return false;
        };
        button.set_tooltip_text(Some(&result_button.title));
        if let Some(image) = button
            .child()
            .and_then(|child| child.downcast::<gtk::Image>().ok())
        {
            let icon = result_button
                .icon
                .as_deref()
                .filter(|icon| !icon.is_empty())
                .unwrap_or("view-more-symbolic");
            set_image_icon(&image, Some(icon), 16);
        }
        child = current.next_sibling();
    }
    child.is_none()
}

fn find_named_child<T>(widget: &impl IsA<gtk::Widget>, name: &str) -> Option<T>
where
    T: IsA<gtk::Widget> + Clone + 'static,
{
    let widget = widget.as_ref();
    let mut child = widget.first_child();
    while let Some(current) = child {
        if current.widget_name() == name {
            if let Ok(found) = current.clone().downcast::<T>() {
                return Some(found);
            }
        }
        if let Some(found) = find_named_child::<T>(&current, name) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

fn result_image(icon: Option<&str>) -> gtk::Image {
    let image = gtk::Image::new();
    set_image_icon(&image, icon, 0);
    image
}

fn set_image_icon(image: &gtk::Image, icon: Option<&str>, pixel_size: i32) {
    let Some(icon) = icon.filter(|icon| !icon.is_empty()) else {
        image.set_icon_name(Some("system-search-symbolic"));
        if pixel_size > 0 {
            image.set_pixel_size(pixel_size);
        }
        return;
    };

    if icon.starts_with('/') {
        image.set_from_file(Some(icon));
    } else {
        image.set_icon_name(Some(icon));
    }
    if pixel_size > 0 {
        image.set_pixel_size(pixel_size);
    }
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
            min-height: 52px;
            transition: background-color 80ms ease-out;
        }
        .result-title {
            font-weight: 600;
        }
        .result-subtitle {
            opacity: 0.72;
            font-size: 12px;
        }
        .result-buttons {
            margin-left: 4px;
        }
        .result-action-button {
            min-width: 28px;
            min-height: 28px;
            padding: 3px;
            border-radius: 8px;
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
