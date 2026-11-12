//! Indexing the chosen folder (`cacheData()`).

use super::*;

impl MainView {
    /// `cacheData()`.
    pub(super) fn cache_data(&mut self, cx: &mut Context<Self>) {
        // A new index: a search or sort still running belongs to the old one, and nothing on screen
        // came from the new one, so any query searches afresh.
        self.debounce = None;
        self.force_pending = false;
        self.sort_task = None;
        self.is_searching = false;
        self.set_results(Vec::new(), Shown::none(), cx);
        self.indexed_count = 0;
        self.index_failed = false;
        let Some(dir) = self.directory.clone() else { return };

        // resetCache()
        if let Some(cancel) = self.index_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.index_task = None;
        // The old walk may still be running on its thread; from here on it adds nothing.
        let epoch = self.index.clear();
        self.row_cache.clear();
        self.is_indexing = true;
        self.sync_options();
        self.index_epoch = epoch;
        let cancel = Arc::new(AtomicBool::new(false));
        self.index_cancel = Some(cancel.clone());

        let (progress_tx, mut progress) = mpsc::unbounded::<usize>();
        let index = self.index.clone();
        let work = cx.background_executor().spawn(async move {
            eprintln!("start caching... {}", dir.display());
            let started = std::time::Instant::now();
            let result = index.index_directory(&dir, epoch, &cancel, |count| {
                let _ = progress_tx.unbounded_send(count);
            });
            if result.is_ok() {
                eprintln!("cache completed. total {} ({:.2?})", index.len(), started.elapsed());
                let skipped = index.skipped_directories();
                if !skipped.is_empty() {
                    eprintln!("Warning: {} directories were skipped:", skipped.len());
                    for (i, dir) in skipped.iter().enumerate() {
                        eprintln!("  {}. {dir}", i + 1);
                    }
                }
            }
            result
        });
        self.index_task = Some(cx.spawn(async move |this, cx| {
            while let Some(count) = progress.next().await {
                let alive = this.update(cx, |this, cx| {
                    if this.index_epoch == epoch {
                        this.indexed_count = count;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.index_epoch != epoch {
                    return;
                }
                this.is_indexing = false;
                this.index_cancel = None;
                this.index_failed = result == Err(IndexError::Unreadable);
                this.focus_search = true;
                // The query still in the field is searched in the new index.
                let query = this.debug_query.take().unwrap_or_else(|| this.search_text.clone());
                if result.is_ok() && !query.is_empty() {
                    this.search_text = query.clone();
                    this.search_all(Some(&query), true, cx);
                }
                this.sync_options();
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn should_show_index_button(&self) -> bool {
        self.results.is_empty()
            && self.search_text.is_empty()
            && self.directory.is_some()
            && !self.is_indexing
            && !self.has_indexed()
    }

    /// The folder is indexed (also when a reopened window finds the index already built).
    pub(super) fn has_indexed(&self) -> bool {
        self.directory.is_some() && self.index.is_cached()
    }
}
