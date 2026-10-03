//! Sticker tabs and navigation. Pack bodies are fetched only when opened.
use crate::store::Sticker;
use crate::{App, TdWorker, actions, send_request, text};
use crossterm::event::KeyCode;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Recent,
    Favorites,
    Set(i64),
    Search,
}
#[derive(Default)]
pub struct Panel {
    pub tab: Tab,
    pub columns: usize,
    pub rows: usize,
    pub top: usize,
    pub query: String,
    pub query_cursor: usize,
    pub submitted: String,
    pub search_focus: bool,
    pub generation: u64,
    pub loading: bool,
    pub preview_visible: bool,
    pub requested_sets: std::collections::HashSet<i64>,
}
impl App {
    pub fn sticker_items(&self) -> &[Sticker] {
        match self.sticker_panel.tab {
            Tab::Recent => &self.store.recent_stickers,
            Tab::Favorites => &self.store.favorite_stickers,
            Tab::Set(id) => self
                .store
                .sticker_sets
                .iter()
                .find(|(key, _)| *key == id)
                .map(|(_, s)| s.as_slice())
                .unwrap_or(&[]),
            Tab::Search => &self.store.sticker_results,
        }
    }
    pub fn sticker_tabs(&self) -> Vec<(Tab, String)> {
        let mut tabs = vec![
            (Tab::Recent, "最近".into()),
            (Tab::Favorites, "收藏".into()),
        ];
        tabs.extend(
            self.store
                .installed_sticker_sets
                .iter()
                .map(|(id, title)| (Tab::Set(*id), title.clone())),
        );
        if self.sticker_panel.tab == Tab::Search {
            tabs.push((Tab::Search, "搜索结果".into()));
        }
        tabs
    }
}
pub fn open(app: &mut App, worker: &TdWorker) {
    app.close_overlays();
    app.sticker_picker = true;
    app.focus_messages = true;
    app.sticker_cursor = 0;
    let generation = app.sticker_panel.generation;
    app.sticker_panel = Panel {
        generation,
        ..Panel::default()
    };
    send_request(app, worker, actions::recent_stickers());
    send_request(
        app,
        worker,
        json!({"@type":"getFavoriteStickers","@extra":"favorite-stickers"}),
    );
    send_request(
        app,
        worker,
        json!({"@type":"getInstalledStickerSets","sticker_type":{"@type":"stickerTypeRegular"},"@extra":"installed-sticker-sets"}),
    );
}
pub fn select_tab(app: &mut App, worker: &TdWorker, tab: Tab) -> bool {
    app.sticker_panel.tab = tab;
    app.sticker_panel.search_focus = false;
    app.sticker_panel.top = 0;
    app.sticker_cursor = 0;
    if let Tab::Set(id) = tab {
        if let Some(index) = app
            .store
            .sticker_sets
            .iter()
            .position(|(key, _)| *key == id)
        {
            let entry = app.store.sticker_sets.remove(index).unwrap();
            app.store.sticker_sets.push_back(entry);
        } else if app.sticker_panel.requested_sets.insert(id)
            && !send_request(
                app,
                worker,
                json!({"@type":"getStickerSet","set_id":id.to_string(),"@extra":format!("sticker-set:{id}")}),
            )
        {
            app.sticker_panel.requested_sets.remove(&id);
        }
    }
    true
}
pub fn cycle_tab(app: &mut App, worker: &TdWorker, delta: isize) -> bool {
    let tabs = app.sticker_tabs();
    let current = tabs
        .iter()
        .position(|(tab, _)| *tab == app.sticker_panel.tab)
        .unwrap_or(0);
    let next = current.saturating_add_signed(delta).min(tabs.len() - 1);
    select_tab(app, worker, tabs[next].0)
}
pub fn search(app: &mut App, worker: &TdWorker) {
    let query = app.sticker_panel.query.trim().to_owned();
    if query.is_empty() {
        select_tab(app, worker, Tab::Recent);
        return;
    }
    app.sticker_panel.generation += 1;
    app.sticker_panel.submitted = query.clone();
    app.store.sticker_search_tag = format!("sticker-search:{}", app.sticker_panel.generation);
    app.store.sticker_results.clear();
    app.sticker_panel.loading = true;
    select_tab(app, worker, Tab::Search);
    let emoji = query
        .chars()
        .any(|c| (c as u32) >= 0x2000 && !('\u{4e00}'..='\u{9fff}').contains(&c));
    if !send_request(
        app,
        worker,
        json!({"@type":"searchStickers","sticker_type":{"@type":"stickerTypeRegular"},
        "emojis":if emoji { &query } else { "" },"query":if emoji { "" } else { &query },
        "input_language_codes":["zh-CN","en"],"offset":0,"limit":64,"@extra":app.store.sticker_search_tag}),
    ) {
        app.sticker_panel.loading = false;
    }
}
pub fn key(app: &mut App, worker: &TdWorker, key: KeyCode) {
    if key == KeyCode::Esc {
        close(app);
        return;
    }
    if key == KeyCode::Tab || key == KeyCode::BackTab {
        app.sticker_panel.search_focus = !app.sticker_panel.search_focus;
        return;
    }
    if app.sticker_panel.search_focus {
        if key == KeyCode::Enter {
            search(app, worker);
        } else if app.sticker_panel.query.len() < 256 || !matches!(key, KeyCode::Char(_)) {
            text::edit(
                &mut app.sticker_panel.query,
                &mut app.sticker_panel.query_cursor,
                key,
            );
        }
        return;
    }
    let cols = app.sticker_panel.columns.max(1);
    let delta = match key {
        KeyCode::Left | KeyCode::Char('h') => -1,
        KeyCode::Right | KeyCode::Char('l') => 1,
        KeyCode::Up | KeyCode::Char('k') => -(cols as isize),
        KeyCode::Down | KeyCode::Char('j') => cols as isize,
        KeyCode::PageUp => -((cols * app.sticker_panel.rows.max(1)) as isize),
        KeyCode::PageDown => (cols * app.sticker_panel.rows.max(1)) as isize,
        KeyCode::Char('[') => {
            cycle_tab(app, worker, -1);
            return;
        }
        KeyCode::Char(']') => {
            cycle_tab(app, worker, 1);
            return;
        }
        KeyCode::Char('f') => {
            favorite(app, worker);
            return;
        }
        KeyCode::Char('/') => {
            app.sticker_panel.search_focus = true;
            return;
        }
        KeyCode::Enter => {
            crate::interaction::perform(app, worker, crate::ui::Action::Confirm);
            return;
        }
        _ => 0,
    };
    app.sticker_cursor = app
        .sticker_cursor
        .saturating_add_signed(delta)
        .min(app.sticker_items().len().saturating_sub(1));
}
pub fn download_visible(app: &mut App, worker: &TdWorker) {
    if !app.sticker_picker || !app.sticker_panel.preview_visible {
        return;
    }
    let start = app.sticker_panel.top * app.sticker_panel.columns.max(1);
    let count = app.sticker_panel.rows.max(1) * app.sticker_panel.columns.max(1);
    let ids: Vec<_> = app
        .sticker_items()
        .iter()
        .skip(start)
        .take(count)
        .filter(|s| s.preview.path.is_none() && s.preview.file_id > 0)
        .map(|s| s.preview.file_id)
        .collect();
    for id in ids {
        if app.requested_files.len() >= 4 {
            break;
        }
        if app.failed_files.contains(&id) || !app.requested_files.insert(id) {
            continue;
        }
        if !send_request(app, worker, actions::download(id)) {
            app.requested_files.remove(&id);
            app.failed_files.insert(id);
        }
    }
}
pub fn on_response(app: &mut App, value: &Value) {
    let extra = value["@extra"].as_str().unwrap_or("");
    if extra == app.store.sticker_search_tag && !extra.is_empty() {
        app.sticker_panel.loading = false;
    }
    if let Some(id) = extra
        .strip_prefix("sticker-set:")
        .and_then(|id| id.parse().ok())
    {
        app.sticker_panel.requested_sets.remove(&id);
    }
    app.sticker_cursor = app
        .sticker_cursor
        .min(app.sticker_items().len().saturating_sub(1));
}

pub fn favorite(app: &mut App, worker: &TdWorker) {
    if let Some(id) = app
        .sticker_items()
        .get(app.sticker_cursor)
        .map(|s| s.file_id)
    {
        let remove = app.store.favorite_stickers.iter().any(|s| s.file_id == id);
        let request = json!({"@type":if remove { "removeFavoriteSticker" } else { "addFavoriteSticker" },"sticker":{"@type":"inputFileId","id":id},"@extra":"refresh-favorites"});
        send_request(app, worker, request);
    }
}

pub fn close(app: &mut App) {
    app.sticker_picker = false;
    if app.store.active_chat.is_some() {
        if app.input_mode == crate::InputMode::Off {
            app.input_mode = crate::InputMode::Send;
        }
        app.focus_messages = true;
        app.composer_focus = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tdlib::{TdCommand, TdEvent};
    fn sticker(id: i32) -> Value {
        json!({"sticker":{"id":id,"local":{"is_downloading_completed":false}},"emoji":"😊","width":128,"height":128,"format":{"@type":"stickerFormatWebp"}})
    }
    #[test]
    fn packs_load_lazily_and_grid_downloads_only_visible_items() {
        let mut app = crate::ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        open(&mut app, &worker);
        let opening: Vec<_> = requests
            .try_iter()
            .filter_map(|r| {
                if let TdCommand::Request(v) = r {
                    Some(v["@type"].as_str().unwrap().to_owned())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            opening,
            vec![
                "getRecentStickers",
                "getFavoriteStickers",
                "getInstalledStickerSets"
            ]
        );
        app.store.apply(&json!({"@type":"stickerSets","@extra":"installed-sticker-sets","sets":[{"id":"9007199254740993","title":"完整包名"}]}));
        let id = 9007199254740993;
        select_tab(&mut app, &worker, Tab::Set(id));
        let TdCommand::Request(request) = requests.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(request["set_id"], id.to_string());
        app.apply(TdEvent::Update(json!({"@type":"stickerSet","@extra":format!("sticker-set:{id}"),"id":id.to_string(),"stickers":(1..=30).map(sticker).collect::<Vec<_>>() })));
        app.sticker_panel.columns = 3;
        app.sticker_panel.rows = 2;
        app.sticker_panel.top = 2;
        download_visible(&mut app, &worker);
        assert!(
            requests.try_recv().is_err(),
            "text-only tiny grid must not download thumbnails"
        );
        app.sticker_panel.preview_visible = true;
        download_visible(&mut app, &worker);
        let ids: Vec<_> = requests
            .try_iter()
            .filter_map(|r| {
                if let TdCommand::Request(v) = r {
                    v["file_id"].as_i64()
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(ids, vec![7, 8, 9, 10]);
        download_visible(&mut app, &worker);
        assert!(
            requests.try_recv().is_err(),
            "visible download concurrency must stay at four"
        );
        select_tab(&mut app, &worker, Tab::Set(id));
        assert!(
            requests.try_recv().is_err(),
            "cached pack should not fetch again"
        );
    }
    #[test]
    fn stale_search_responses_do_not_replace_new_results_and_grid_keys_use_columns() {
        let mut app = crate::ui::tests::fixture();
        let (worker, _) = TdWorker::test_pair();
        app.sticker_picker = true;
        app.sticker_panel.query = "猫".into();
        search(&mut app, &worker);
        let old = app.store.sticker_search_tag.clone();
        app.sticker_panel.query = "狗".into();
        search(&mut app, &worker);
        let current = app.store.sticker_search_tag.clone();
        app.apply(TdEvent::Update(json!({"@type":"stickers","@extra":current,"stickers":(1..=12).map(sticker).collect::<Vec<_>>() })));
        app.apply(TdEvent::Update(
            json!({"@type":"stickers","@extra":old,"stickers":[sticker(999)]}),
        ));
        assert_eq!(app.sticker_items().len(), 12);
        app.sticker_panel.columns = 3;
        key(&mut app, &worker, KeyCode::Down);
        assert_eq!(app.sticker_cursor, 3);
        key(&mut app, &worker, KeyCode::Right);
        assert_eq!(app.sticker_cursor, 4);
        key(&mut app, &worker, KeyCode::Up);
        assert_eq!(app.sticker_cursor, 1);
        for id in 1..=6 {
            app.store
                .apply(&json!({"@type":"stickerSet","id":id,"stickers":[sticker(id)]}));
        }
        assert_eq!(app.store.sticker_sets.len(), 4);
    }
}
