//! Searching, sorting, and keeping the selected item in view as the results change.

use super::*;

impl MainView {
    pub(super) fn search_options(&self) -> SearchOptions {
        SearchOptions {
            by_folder: self.settings.by_folder,
            by_folder_double: self.settings.by_folder_double,
            sort: self.settings.sort,
            reverse: self.settings.reverse,
            fresh: false,
        }
    }

    /// `searchAll(for:isForce:)` with the 100 ms debounce.
    pub fn search_all(&mut self, input: Option<&str>, force: bool, cx: &mut Context<Self>) {
        if self.directory.is_none() {
            self.terminal_output = tr("Please select a path first").to_string();
            cx.notify();
            return;
        }
        quick_look_close();
        if let Some(input) = input.filter(|i| !i.is_empty()) {
            self.search_text = input.to_string();
        }
        // A forced search (after a file operation or an option change) that has not landed yet is
        // replaced by this one, so this one is forced too: the results must still be refreshed.
        let force = force || self.force_pending;
        self.force_pending = force;
        self.debounce = Some(cx.spawn(async move |this, cx| {
            if !force {
                cx.background_executor().timer(Duration::from_millis(100)).await;
            }
            let Ok((text, current, shown, options, convert, directory, index)) = this.update(cx, |this, cx| {
                this.is_searching = true;
                this.terminal_output.clear();
                cx.notify();
                (
                    this.search_text.clone(),
                    this.results.clone(),
                    this.shown.clone(),
                    SearchOptions {
                        fresh: force,
                        ..this.search_options()
                    },
                    this.settings.convert_full_width,
                    this.directory.clone(),
                    this.index.clone(),
                )
            }) else {
                return;
            };
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    match plan(&text, convert, force, &shown) {
                        Plan::Unchanged => SearchOutcome::Unchanged,
                        Plan::Clear => SearchOutcome::Results(Vec::new(), Shown::none()),
                        Plan::Search { query, strings } => {
                            let started = std::time::Instant::now();
                            let label = cfg!(debug_assertions).then(|| format!("{strings:?}"));
                            let (results, shown) =
                                run(&index, query, strings, options, directory.as_deref(), &current, &shown);
                            if let Some(label) = label {
                                eprintln!("search {label}: {} items in {:.2?}", results.len(), started.elapsed());
                            }
                            SearchOutcome::Results(results, shown)
                        }
                    }
                })
                .await;
            // Dropped before here (a newer search or a re-sort replaced this task), nothing of this
            // search is kept: `shown` still describes what is on screen.
            let _ = this.update(cx, |this, cx| {
                if let SearchOutcome::Results(results, shown) = outcome {
                    this.set_results(results, shown, cx);
                    // The sort changed while this ran: a re-sort no longer cancels a search.
                    if (options.sort, options.reverse) != (this.settings.sort, this.settings.reverse) {
                        this.resort(cx);
                    }
                }
                this.force_pending = false;
                this.is_searching = false;
                this.terminal_output.clear();
                cx.notify();
            });
        }));
    }

    /// New results (a search, a re-sort, a refresh after a file operation). Scrolling follows the
    /// selection: a selected item still present stays where it was on screen (or, if it was out of
    /// sight, comes into view at the top); with none, the list starts from the top.
    pub(super) fn set_results(&mut self, results: Vec<Arc<FileInfo>>, shown: Shown, cx: &mut Context<Self>) {
        let mut on_screen = self.selected_on_screen();
        let renamed = std::mem::take(&mut self.renamed);
        if !renamed.is_empty() {
            // A renamed item is still the one selected, at the same place.
            let follow = |path: &Arc<str>| renamed.get(path).cloned().unwrap_or_else(|| path.clone());
            on_screen = on_screen.iter().map(|(path, top)| (follow(path), *top)).collect();
            self.selection = self.selection.iter().map(follow).collect();
            self.anchor = self.anchor.as_ref().map(follow);
            self.lead = self.lead.as_ref().map(follow);
        }
        self.positions = results.iter().enumerate().map(|(i, r)| (r.path.clone(), i)).collect();
        self.results = Arc::new(results);
        self.shown = shown;
        // Keep whatever is still present selected.
        let positions = &self.positions;
        self.selection.retain(|p| positions.contains_key(p));
        if self.anchor.as_ref().is_some_and(|a| !positions.contains_key(a)) {
            self.anchor = None;
        }
        if self.lead.as_ref().is_some_and(|l| !positions.contains_key(l)) {
            self.lead = None;
        }
        self.context_row = None;
        if self.row_cache.len() > 20_000 {
            self.row_cache.clear();
        }
        let count = self.results.len();
        self.list_state
            .reset_with_uniform_height(count, self.row_estimate.borrow().hint());
        self.pending_anchor = None;
        if let Some((ix, path)) = self.item_to_keep_in_view(&on_screen) {
            let pad = px(ROW_INSET);
            let want = on_screen.get(&path).copied().unwrap_or(pad);
            // Off-screen rows only have estimated heights yet: this lands close, and
            // `settle_anchor` corrects it once the rows around it are measured.
            self.list_state.scroll_to(ListOffset {
                item_ix: ix,
                offset_in_item: px(0.),
            });
            self.list_state.scroll_by(pad - want);
            self.pending_anchor = Some(PendingAnchor { path, want, tries: 3 });
        }
        cx.notify();
    }

    /// Selected rows on screen now, with their top relative to the list's top.
    pub(super) fn selected_on_screen(&self) -> HashMap<Arc<str>, Pixels> {
        let viewport = self.list_state.viewport_bounds();
        let mut rows = HashMap::new();
        for ix in self.list_state.logical_scroll_top().item_ix..self.results.len() {
            let Some(bounds) = self.item_bounds(ix) else { break };
            if bounds.top() >= viewport.bottom() {
                break;
            }
            let path = &self.results[ix].path;
            if bounds.bottom() > viewport.top() && self.selection.contains(path) {
                rows.insert(path.clone(), bounds.top() - viewport.top());
            }
        }
        rows
    }

    /// The selected item (now in `results`) to keep in sight: the one last clicked or moved to,
    /// else the first that was on screen, else the first in the list.
    pub(super) fn item_to_keep_in_view(&self, on_screen: &HashMap<Arc<str>, Pixels>) -> Option<(usize, Arc<str>)> {
        let at = |path: &Arc<str>| self.positions.get(path).map(|&ix| (ix, path.clone()));
        let lead = self.lead.as_ref().filter(|l| self.selection.contains(*l)).and_then(at);
        lead.or_else(|| {
            let mut visible: Vec<(&Arc<str>, &Pixels)> = on_screen.iter().collect();
            visible.sort_by(|a, b| f32::from(*a.1).total_cmp(&f32::from(*b.1)));
            visible.into_iter().find_map(|(path, _)| at(path))
        })
        .or_else(|| self.selection.iter().filter_map(at).min_by_key(|(ix, _)| *ix))
    }

    /// Finishes `set_results`' placement once the rows around the kept item have been measured.
    pub(super) fn settle_anchor(&mut self, window: &mut Window) {
        let Some(PendingAnchor { path, want, tries }) = self.pending_anchor.take() else {
            return;
        };
        let Some(&ix) = self.positions.get(&path) else { return };
        // `None` until the new rows have been laid out.
        let off = self
            .item_bounds(ix)
            .map(|bounds| bounds.top() - self.list_state.viewport_bounds().top() - want);
        if off.is_some_and(|off| off.abs() <= px(0.5)) || tries == 0 {
            return;
        }
        if let Some(off) = off {
            self.list_state.scroll_by(off);
        }
        self.pending_anchor = Some(PendingAnchor {
            path,
            want,
            tries: tries - 1,
        });
        window.request_animation_frame();
    }

    /// `SortManagerView.sort()` — re-sorts what is shown. Swift's binding also cancelled a pending
    /// search, which could drop the re-search an option change had just started; here the search
    /// goes on and is re-sorted when it lands.
    pub(super) fn resort(&mut self, cx: &mut Context<Self>) {
        let on_screen = self.results.clone();
        let (sort, reverse) = (self.settings.sort, self.settings.reverse);
        self.sort_task = Some(cx.spawn(async move |this, cx| {
            let unsorted = on_screen.clone();
            let sorted = cx
                .background_executor()
                .spawn(async move {
                    let mut items: Vec<Arc<FileInfo>> = (*unsorted).clone();
                    sort_file_infos(&mut items, sort, reverse);
                    items
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                // Newer results arrived meanwhile: they were sorted (or re-sorted) for themselves.
                if Arc::ptr_eq(&this.results, &on_screen) {
                    let shown = this.shown.clone();
                    this.set_results(sorted, shown, cx);
                }
            });
        }));
    }

    pub(super) fn set_sort(&mut self, sort: SortOption, cx: &mut Context<Self>) {
        if self.settings.sort == sort {
            return;
        }
        self.settings.sort = sort;
        self.settings.save();
        self.resort(cx);
    }

    pub(super) fn toggle_reverse(&mut self, cx: &mut Context<Self>) {
        self.settings.reverse = !self.settings.reverse;
        self.settings.save();
        self.resort(cx);
    }
}
