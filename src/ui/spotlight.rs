use crate::config::ConfigStore;
use crate::i18n::I18n;
use crate::plugin::{
    activate_action, activate_result, SearchResult, SearchResultButton, SharedRegistry,
};
use crate::theme;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

struct RefreshTimer {
    source: glib::SourceId,
    deadline: Instant,
}

struct RenderedRow {
    row: gtk::ListBoxRow,
    result: Rc<RefCell<SearchResult>>,
}

pub struct SpotlightWindow {
    window: gtk::Window,
    entry: gtk::SearchEntry,
    panel: gtk::Box,
    results_view: gtk::Overlay,
    list: gtk::ListBox,
    rows: Rc<RefCell<Vec<RenderedRow>>>,
    scroll_indicator: gtk::DrawingArea,
    results: Rc<RefCell<Vec<SearchResult>>>,
    result_offset: Rc<RefCell<usize>>,
    selected_index: Rc<RefCell<usize>>,
    refresh_sources: Rc<RefCell<HashMap<String, RefreshTimer>>>,
    search_delay: Rc<RefCell<Option<glib::SourceId>>>,
    generation: Rc<Cell<u64>>,
    search_pending: Rc<Cell<bool>>,
    refresh_pending: Rc<RefCell<HashSet<String>>>,
    worker_running: Rc<Cell<bool>>,
    config: Rc<RefCell<ConfigStore>>,
    plugins: SharedRegistry,
    ensure_plugins_loaded: Rc<dyn Fn()>,
}

impl SpotlightWindow {
    pub fn new(
        app: &impl IsA<gtk::Application>,
        i18n: Rc<I18n>,
        config: Rc<RefCell<ConfigStore>>,
        plugins: SharedRegistry,
        ensure_plugins_loaded: Rc<dyn Fn()>,
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

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 6);
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
        let refresh_sources = Rc::new(RefCell::new(HashMap::new()));

        let list = gtk::ListBox::new();
        list.add_css_class("spotlight-results");
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_hexpand(true);
        list.set_visible(false);

        let scroll_indicator = gtk::DrawingArea::new();
        scroll_indicator.add_css_class("result-scroll-indicator");
        scroll_indicator.set_content_width(6);
        scroll_indicator.set_hexpand(false);
        scroll_indicator.set_halign(gtk::Align::End);
        scroll_indicator.set_valign(gtk::Align::Fill);
        scroll_indicator.set_can_target(false);
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

        let results_view = gtk::Overlay::new();
        results_view.add_css_class("results-view");
        results_view.set_hexpand(true);
        results_view.set_child(Some(&list));
        results_view.add_overlay(&scroll_indicator);
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
            rows: Rc::new(RefCell::new(Vec::new())),
            scroll_indicator,
            results,
            result_offset,
            selected_index,
            refresh_sources,
            search_delay: Rc::new(RefCell::new(None)),
            generation: Rc::new(Cell::new(0)),
            search_pending: Rc::new(Cell::new(false)),
            refresh_pending: Rc::new(RefCell::new(HashSet::new())),
            worker_running: Rc::new(Cell::new(false)),
            config,
            plugins,
            ensure_plugins_loaded,
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
            self.cancel_pending_work();
        } else {
            self.present();
        }
    }

    pub fn present(&self) {
        self.cancel_pending_work();
        self.entry.set_text("");
        self.results.borrow_mut().clear();
        self.render_results(&[], 0, 0, 0);
        // Present the window immediately so it appears without delay, then load
        // search results on the next event-loop iteration.
        self.window.present();
        self.entry.grab_focus();
        let spotlight = self.clone_handles();
        glib::idle_add_local_once(move || {
            if spotlight.window.is_visible() {
                (spotlight.ensure_plugins_loaded)();
                spotlight.refresh_results(&spotlight.entry.text());
            }
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
        self.render_results(
            &self.results.borrow(),
            *self.result_offset.borrow(),
            *self.selected_index.borrow(),
            cfg.max_visible_results,
        );
    }

    fn connect_signals(&self) {
        let spotlight = self.clone_handles();
        // Built-in results are cheap and should follow every keystroke. Only
        // external commands are debounced, independently of GTK's search delay.
        self.entry.connect_changed(move |entry| {
            if spotlight.window.is_visible() {
                spotlight.refresh_results(&entry.text());
            }
        });

        let spotlight = self.clone_handles();
        self.window.connect_visible_notify(move |window| {
            if !window.is_visible() {
                spotlight.cancel_pending_work();
            }
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
        let results = self.results.clone();
        let result_offset = self.result_offset.clone();
        let selected_index = self.selected_index.clone();
        let config = self.config.clone();
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
                    spotlight.render_results(
                        &results_ref,
                        next_offset,
                        next_selected,
                        config.borrow().current().window.max_visible_results,
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
                    spotlight.render_results(
                        &results_ref,
                        next_offset,
                        next_selected,
                        config.borrow().current().window.max_visible_results,
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

    fn cancel_refreshes(&self) {
        for (_, timer) in self.refresh_sources.borrow_mut().drain() {
            timer.source.remove();
        }
        self.refresh_pending.borrow_mut().clear();
    }

    fn cancel_pending_work(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        if let Some(source) = self.search_delay.borrow_mut().take() {
            source.remove();
        }
        self.search_pending.set(false);
        self.cancel_refreshes();
    }

    fn refresh_results(&self, query: &str) {
        self.cancel_pending_work();
        (self.ensure_plugins_loaded)();
        let found = {
            let config = self.config.borrow();
            let plugins = self.plugins.borrow();
            plugins.rank_results(
                config.current(),
                plugins.search_immediate(config.current(), query),
            )
        };
        self.apply_results(found, false);

        let spotlight = self.clone_handles();
        let source = glib::timeout_add_local_once(Duration::from_millis(60), move || {
            spotlight.search_delay.borrow_mut().take();
            spotlight.search_pending.set(true);
            spotlight.start_worker();
        });
        *self.search_delay.borrow_mut() = Some(source);
    }

    fn apply_results(&self, found: Vec<SearchResult>, preserve_selection: bool) {
        let selected_key = preserve_selection
            .then(|| {
                self.results
                    .borrow()
                    .get(*self.selected_index.borrow())
                    .map(SearchResult::usage_key)
            })
            .flatten();
        let selected = selected_key
            .and_then(|key| found.iter().position(|result| result.usage_key() == key))
            .unwrap_or(0);
        let max_visible = self.config.borrow().current().window.max_visible_results;
        let visible = visible_result_count(found.len(), max_visible);
        let offset = if preserve_selection && visible > 0 {
            offset_for_selection(
                (*self.result_offset.borrow()).min(found.len().saturating_sub(visible)),
                selected,
                visible,
            )
        } else {
            0
        };
        *self.result_offset.borrow_mut() = offset;
        *self.selected_index.borrow_mut() = selected;
        *self.results.borrow_mut() = found;
        self.render_results(&self.results.borrow(), offset, selected, max_visible);
    }

    fn start_worker(&self) {
        if self.worker_running.replace(true) {
            return;
        }
        let spotlight = self.clone_handles();
        glib::spawn_future_local(async move {
            loop {
                if !spotlight.window.is_visible() {
                    break;
                }
                let plugin_id = if spotlight.search_pending.replace(false) {
                    spotlight.refresh_pending.borrow_mut().clear();
                    None
                } else {
                    let next = spotlight.refresh_pending.borrow().iter().next().cloned();
                    let Some(id) = next else { break };
                    spotlight.refresh_pending.borrow_mut().remove(&id);
                    Some(id)
                };
                let generation = spotlight.generation.get();
                let query = spotlight.entry.text().to_string();
                let config = spotlight.config.borrow().current().clone();
                let registry = spotlight.plugins.borrow().clone();
                let refresh_id = plugin_id.clone();
                let refresh_query = plugin_id.as_ref().and_then(|id| {
                    spotlight
                        .config
                        .borrow()
                        .plugin_query(id, &query)
                        .or_else(|| query.trim().is_empty().then_some(""))
                        .map(str::to_owned)
                });
                // At most one query batch is active. New input replaces pending
                // work; the GTK loop never waits for child processes or joins.
                let found = gio::spawn_blocking(move || {
                    if let Some(id) = refresh_id {
                        refresh_query
                            .map(|query| registry.search_plugin_with_config(&config, &id, &query))
                            .unwrap_or_default()
                    } else {
                        registry.search_blocking(&config, &query)
                    }
                })
                .await;
                if generation != spotlight.generation.get() || !spotlight.window.is_visible() {
                    continue;
                }
                let found = match found {
                    Ok(found) => found,
                    Err(_) => {
                        tracing::warn!("background plugin search panicked");
                        Vec::new()
                    }
                };
                let mut merged = if let Some(id) = plugin_id {
                    spotlight
                        .results
                        .borrow()
                        .iter()
                        .filter(|result| result.source_plugin_id.as_deref() != Some(id.as_str()))
                        .cloned()
                        .collect::<Vec<_>>()
                } else {
                    spotlight.plugins.borrow().search_immediate(
                        spotlight.config.borrow().current(),
                        &spotlight.entry.text(),
                    )
                };
                merged.extend(
                    found.into_iter().filter(|result| {
                        !spotlight.entry.text().trim().is_empty() || result.pinned
                    }),
                );
                let merged = spotlight
                    .plugins
                    .borrow()
                    .rank_results(spotlight.config.borrow().current(), merged);
                spotlight.apply_results(merged, true);
            }
            spotlight.worker_running.set(false);
        });
    }

    fn render_results(
        &self,
        results: &[SearchResult],
        offset: usize,
        selected_index: usize,
        max_visible_results: i32,
    ) {
        let visible_count = visible_result_count(results.len(), max_visible_results);
        let mut rows = self.rows.borrow_mut();
        while rows.len() > visible_count {
            if let Some(row) = rows.pop() {
                self.list.remove(&row.row);
            }
        }
        for (index, result) in results.iter().skip(offset).take(visible_count).enumerate() {
            if let Some(rendered) = rows.get(index) {
                let previous = rendered.result.replace(result.clone());
                if previous != *result
                    && !update_result_row_content(&rendered.row, result, &previous)
                {
                    rendered.row.set_child(Some(&result_row_content(
                        &rendered.result,
                        &self.window,
                        &self.config,
                        self,
                    )));
                }
            } else {
                let binding = Rc::new(RefCell::new(result.clone()));
                let row = result_row(&binding, &self.window, &self.config, self);
                self.list.append(&row);
                rows.push(RenderedRow {
                    row,
                    result: binding,
                });
            }
        }
        drop(rows);
        // Stop refreshes for plugins scrolled out of view.
        let visible_plugins: HashSet<_> = results
            .iter()
            .skip(offset)
            .take(visible_count)
            .filter(|result| result.refresh_interval_ms.is_some())
            .filter_map(|result| result.source_plugin_id.as_deref())
            .collect();
        let hidden: Vec<_> = self
            .refresh_sources
            .borrow()
            .keys()
            .filter(|id| !visible_plugins.contains(id.as_str()))
            .cloned()
            .collect();
        for id in hidden {
            if let Some(timer) = self.refresh_sources.borrow_mut().remove(&id) {
                timer.source.remove();
            }
            self.refresh_pending.borrow_mut().remove(&id);
        }
        let has_results = visible_count > 0;
        self.list.set_visible(has_results);
        self.results_view.set_visible(has_results);
        update_selection(&self.list, offset, selected_index);
        update_scroll_indicator(&self.scroll_indicator, results.len(), visible_count);

        let mut refreshes: HashMap<&str, (&SearchResult, u64)> = HashMap::new();
        for result in results.iter().skip(offset).take(visible_count) {
            if let (Some(id), Some(interval), Some(_)) = (
                &result.source_plugin_id,
                result.refresh_interval_ms,
                &result.refresh_key,
            ) {
                refreshes
                    .entry(id.as_str())
                    .and_modify(|(_, shortest)| *shortest = (*shortest).min(interval))
                    .or_insert((result, interval));
            }
        }
        for (result, interval) in refreshes.into_values() {
            self.refresh_result_after(result, Duration::from_millis(interval), false);
        }
    }

    fn refresh_result_after(&self, result: &SearchResult, delay: Duration, replace_pending: bool) {
        let Some(plugin_id) = result.source_plugin_id.clone() else {
            return;
        };
        if result.refresh_key.is_none() || !self.window.is_visible() {
            return;
        }
        // All visible rows from one plugin share one query and one timer.
        let delay = delay.clamp(
            Duration::from_millis(100),
            Duration::from_millis(u32::MAX as u64),
        );
        let deadline = Instant::now() + delay;
        let pending_timer = self.refresh_sources.borrow_mut().remove(&plugin_id);
        if let Some(timer) = pending_timer {
            if !replace_pending && timer.deadline <= deadline {
                self.refresh_sources.borrow_mut().insert(plugin_id, timer);
                return;
            }
            timer.source.remove();
        }
        let spotlight = self.clone_handles();
        let id = plugin_id.clone();
        let source =
            glib::timeout_add_local_once(delay.max(Duration::from_millis(100)), move || {
                spotlight.refresh_sources.borrow_mut().remove(&id);
                if spotlight.window.is_visible() {
                    spotlight.refresh_pending.borrow_mut().insert(id);
                    spotlight.start_worker();
                }
            });
        self.refresh_sources
            .borrow_mut()
            .insert(plugin_id, RefreshTimer { source, deadline });
    }

    fn clone_handles(&self) -> Self {
        Self {
            window: self.window.clone(),
            entry: self.entry.clone(),
            panel: self.panel.clone(),
            results_view: self.results_view.clone(),
            list: self.list.clone(),
            rows: self.rows.clone(),
            scroll_indicator: self.scroll_indicator.clone(),
            results: self.results.clone(),
            result_offset: self.result_offset.clone(),
            selected_index: self.selected_index.clone(),
            refresh_sources: self.refresh_sources.clone(),
            search_delay: self.search_delay.clone(),
            generation: self.generation.clone(),
            search_pending: self.search_pending.clone(),
            refresh_pending: self.refresh_pending.clone(),
            worker_running: self.worker_running.clone(),
            config: self.config.clone(),
            plugins: self.plugins.clone(),
            ensure_plugins_loaded: self.ensure_plugins_loaded.clone(),
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
    indicator.set_visible(total > visible && visible > 0);
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
    result: &Rc<RefCell<SearchResult>>,
    window: &gtk::Window,
    config: &Rc<RefCell<ConfigStore>>,
    spotlight: &SpotlightWindow,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_hexpand(true);
    row.set_height_request(58);
    row.set_child(Some(&result_row_content(result, window, config, spotlight)));
    row
}

fn result_row_content(
    binding: &Rc<RefCell<SearchResult>>,
    window: &gtk::Window,
    config: &Rc<RefCell<ConfigStore>>,
    spotlight: &SpotlightWindow,
) -> gtk::Box {
    let result = binding.borrow();
    let outer = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    outer.add_css_class("result-row");
    outer.set_hexpand(true);
    outer.set_height_request(52);

    let image = result_image(result.icon.as_deref());
    image.set_pixel_size(32);
    image.set_widget_name("result-icon");
    image.set_valign(gtk::Align::Center);

    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_hexpand(true);
    labels.set_halign(gtk::Align::Fill);
    labels.set_valign(gtk::Align::Center);
    labels.set_width_request(1);
    let title = gtk::Label::new(Some(&result.title));
    title.set_halign(gtk::Align::Fill);
    title.set_hexpand(true);
    title.set_width_chars(1);
    title.set_max_width_chars(1);
    title.set_single_line_mode(true);
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("result-title");
    title.set_widget_name("result-title");

    let subtitle_text = if result.subtitle.is_empty() {
        " "
    } else {
        &result.subtitle
    };
    let subtitle = gtk::Label::new(Some(subtitle_text));
    subtitle.set_halign(gtk::Align::Fill);
    subtitle.set_hexpand(true);
    subtitle.set_width_chars(1);
    subtitle.set_max_width_chars(1);
    subtitle.set_single_line_mode(true);
    subtitle.set_xalign(0.0);
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
        buttons.set_width_request(result_buttons_width(result.buttons.len()));
        for (index, button) in result.buttons.iter().enumerate() {
            let action_button = result_action_button(button);
            let binding = binding.clone();
            let window = window.clone();
            let config = config.clone();
            let spotlight = spotlight.clone_handles();
            action_button.connect_clicked(move |_| {
                let result = binding.borrow().clone();
                let Some(button) = result.buttons.get(index) else {
                    return;
                };
                activate_button_with_usage(&result, button, window.upcast_ref(), &config);
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

fn result_buttons_width(button_count: usize) -> i32 {
    if button_count == 0 {
        0
    } else {
        button_count as i32 * 32 + button_count.saturating_sub(1) as i32 * 4
    }
}

fn update_result_row_content(
    row: &gtk::ListBoxRow,
    result: &SearchResult,
    previous: &SearchResult,
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
        != result.buttons.is_empty();
    if !buttons_match {
        return false;
    }
    if let Some(buttons) = outer
        .last_child()
        .and_then(|child| child.downcast::<gtk::Box>().ok())
        .filter(|buttons| buttons.has_css_class("result-buttons"))
    {
        if result.buttons != previous.buttons && !update_result_buttons(&buttons, &result.buttons) {
            return false;
        }
    }

    if result.icon != previous.icon {
        if let Some(image) = find_named_child::<gtk::Image>(&outer, "result-icon") {
            set_image_icon(&image, result.icon.as_deref(), 32);
        }
    }
    if let Some(title) = find_named_child::<gtk::Label>(&outer, "result-title") {
        title.set_text(&result.title);
    }
    if let Some(subtitle) = find_named_child::<gtk::Label>(&outer, "result-subtitle") {
        let subtitle_text = if result.subtitle.is_empty() {
            " "
        } else {
            &result.subtitle
        };
        subtitle.set_text(subtitle_text);
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

    if let Some(svg) = icon.strip_prefix("data:image/svg+xml,") {
        let svg = percent_decode(svg);
        let bytes = glib::Bytes::from_owned(svg.into_bytes());
        image.set_paintable(Some(&gtk::Svg::from_bytes(&bytes)));
    } else if icon.starts_with('/') {
        image.set_from_file(Some(icon));
    } else {
        image.set_icon_name(Some(icon));
    }
    if pixel_size > 0 {
        image.set_pixel_size(pixel_size);
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                decoded.push(high * 16 + low);
                index += 3;
                continue;
            }
        }

        decoded.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
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
            border: 1px solid alpha(@theme_fg_color, 0.15);
            border-radius: 22px;
            padding: 12px;
            box-shadow: 0 18px 48px alpha(black, 0.30), 0 2px 8px alpha(black, 0.12);
        }
        window.system-light .spotlight-panel {
            background: rgba(250, 250, 252, 0.94);
            color: rgb(36, 36, 36);
        }
        window.system-dark .spotlight-panel {
            background: rgba(30, 31, 36, 0.94);
            color: rgb(238, 238, 238);
        }
        .spotlight-entry {
            min-height: 52px;
            font-size: 21px;
            border-radius: 14px;
            padding-left: 12px;
            padding-right: 12px;
            background: alpha(@theme_fg_color, 0.06);
            box-shadow: none;
            border: 1px solid alpha(@theme_fg_color, 0.08);
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
            border-radius: 12px;
            margin-top: 3px;
            transition: background-color 80ms ease-out;
        }
        .spotlight-results row:first-child {
            margin-top: 3px;
        }
        .spotlight-results row:selected,
        .spotlight-results row:selected:hover,
        .spotlight-results row:selected:focus {
            background: alpha(@accent_bg_color, 0.22);
            border-radius: 12px;
        }
        .spotlight-results row:selected .result-row {
            border-radius: 10px;
        }
        .result-row {
            padding: 4px 12px;
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

    // GTK 4.24 translates backdrop-filter into ext-background-effect-v1.
    // Older supported GTK versions keep the more opaque surface above.
    let blur_provider = gtk::CssProvider::new();
    if gtk::major_version() > 4 || gtk::minor_version() >= 24 {
        blur_provider.load_from_string(
            r#"
            .spotlight-panel { backdrop-filter: blur(24px); }
            window.system-light .spotlight-panel { background: rgba(250, 250, 252, 0.78); }
            window.system-dark .spotlight-panel { background: rgba(30, 31, 36, 0.78); }
            "#,
        );
    }

    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        gtk::style_context_add_provider_for_display(
            &display,
            &blur_provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::plugin::{PluginAction, PluginRegistry, SearchPlugin};
    use std::sync::{Arc, Mutex};

    struct TestPlugin {
        blocking: bool,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl SearchPlugin for TestPlugin {
        fn id(&self) -> &str {
            if self.blocking {
                "slow"
            } else {
                "fast"
            }
        }
        fn name(&self) -> &str {
            self.id()
        }
        fn description(&self) -> &str {
            self.id()
        }
        fn is_blocking(&self) -> bool {
            self.blocking
        }
        fn query(&self, query: &str) -> Vec<SearchResult> {
            self.calls.lock().unwrap().push(query.to_string());
            if self.blocking {
                std::thread::sleep(Duration::from_millis(180));
            }
            let result = SearchResult {
                title: format!("{}:{query}", self.id()),
                subtitle: String::new(),
                icon: None,
                pinned: true,
                action: PluginAction::CopyText(format!("{}:{query}", self.id())),
                buttons: Vec::new(),
                refresh_key: self.blocking.then(|| "live".to_owned()),
                refresh_interval_ms: None,
                source_plugin_id: None,
            };
            let mut results = vec![result.clone()];
            if self.blocking {
                let mut unpinned = result;
                unpinned.pinned = false;
                unpinned.title = format!("unpinned:{query}");
                unpinned.refresh_key = None;
                results.push(unpinned);
            }
            results
        }
    }

    async fn wait_until(mut ready: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !ready() {
            assert!(
                std::time::Instant::now() < deadline,
                "UI work did not complete"
            );
            glib::timeout_future(Duration::from_millis(5)).await;
        }
    }

    #[test]
    #[ignore = "requires a display; run with xvfb-run and --test-threads=1"]
    fn async_search_keeps_ui_responsive_and_discards_stale_results() {
        gtk::init().unwrap();
        let app = gtk::Application::builder()
            .application_id("io.github.gpotlight.Test")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut registry = PluginRegistry::default();
        registry.register(TestPlugin {
            blocking: false,
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        registry.register(TestPlugin {
            blocking: true,
            calls: calls.clone(),
        });
        let spotlight = SpotlightWindow::new(
            &app,
            Rc::new(I18n::load("en-US")),
            Rc::new(RefCell::new(ConfigStore::from_config_for_test(
                AppConfig::default(),
            ))),
            Rc::new(RefCell::new(registry)),
            Rc::new(|| {}),
        );
        glib::MainContext::default().block_on(async {
            spotlight.present();
            wait_until(|| !calls.lock().unwrap().is_empty()).await;
            // Change input repeatedly while the previous subprocess is busy.
            spotlight.entry.set_text("old");
            spotlight.entry.set_text("new");
            assert_eq!(spotlight.results.borrow()[0].title, "fast:new");
            let immediate_row = spotlight.list.row_at_index(0).unwrap();
            let ticks = Rc::new(Cell::new(0));
            let counter = ticks.clone();
            let timer = glib::timeout_add_local(Duration::from_millis(10), move || {
                counter.set(counter.get() + 1);
                glib::ControlFlow::Continue
            });
            wait_until(|| {
                spotlight
                    .results
                    .borrow()
                    .iter()
                    .any(|r| r.title == "slow:new")
            })
            .await;
            timer.remove();
            assert!(ticks.get() >= 5, "slow plugins blocked GTK timers");
            assert!(spotlight
                .results
                .borrow()
                .iter()
                .all(|r| r.title.ends_with(":new")));
            assert!(!calls.lock().unwrap().iter().any(|query| query == "old"));
            assert_eq!(
                spotlight.list.row_at_index(0).unwrap(),
                immediate_row,
                "unchanged rows should be reused"
            );

            // A single-plugin refresh must keep the empty-query pinned filter.
            spotlight.entry.set_text("");
            wait_until(|| {
                spotlight
                    .results
                    .borrow()
                    .iter()
                    .any(|r| r.title == "slow:")
            })
            .await;
            let refreshable = spotlight.results.borrow()[1].clone();
            let before = calls.lock().unwrap().len();
            spotlight.refresh_result_after(&refreshable, Duration::from_millis(100), false);
            wait_until(|| calls.lock().unwrap().len() > before).await;
            wait_until(|| !spotlight.worker_running.get()).await;
            assert!(spotlight.results.borrow().iter().all(|r| r.pinned));

            // Refreshes are grouped by plugin and removed as soon as we hide.
            let mut refreshable = spotlight.results.borrow()[1].clone();
            refreshable.refresh_key = Some("one".into());
            spotlight.refresh_result_after(&refreshable, Duration::from_secs(1), false);
            refreshable.refresh_key = Some("two".into());
            spotlight.refresh_result_after(&refreshable, Duration::from_secs(1), false);
            assert_eq!(spotlight.refresh_sources.borrow().len(), 1);
            let previous_deadline = spotlight.refresh_sources.borrow()["slow"].deadline;
            spotlight.refresh_result_after(&refreshable, Duration::from_millis(100), false);
            assert!(
                spotlight.refresh_sources.borrow()["slow"].deadline < previous_deadline,
                "rows sharing a plugin must use the shortest refresh interval"
            );
            spotlight.window.set_visible(false);
            assert!(spotlight.refresh_sources.borrow().is_empty());
            assert!(spotlight.search_delay.borrow().is_none());
        });
        spotlight.window.destroy();
    }
}
