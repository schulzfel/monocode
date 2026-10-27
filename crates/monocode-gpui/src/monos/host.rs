//! What a Mono does for the sessions it started: groups them in its sidebar
//! folder, and reports back once a group of them has finished.
use super::*;

impl Monos {
    pub(super) fn group_launch(&mut self, id: &str, session: i64, cx: &mut Context<Self>) {
        let Some(mono) = self
            .roster
            .get(id)
            .filter(|m| m.show_started_sessions_in_sidebar && m.use_sidebar_folders)
            .cloned()
        else {
            return;
        };
        let Some(launch) = mono.launches.iter().find(|l| l.session_id == session) else {
            return;
        };
        let project = launch.cwd.clone();
        let mut groups = self
            .owner
            .upgrade()
            .map(|o| o.read(cx).mono_groups(&project))
            .unwrap_or_default();
        let folder = groups
            .folders
            .iter()
            .find(|f| f.name == mono.label())
            .map(|f| f.id.clone())
            .or_else(|| groups.create_folder(&[session], mono::new_id()));
        if let Some(folder) = folder {
            groups.rename_folder(&folder, &mono.label());
            let color = mono::COLORS_MONO
                .iter()
                .position(|c| *c == mono.color)
                .map(|n| if n == 8 { 6 } else { n + 1 });
            groups.set_color(&folder, color);
            groups.add_to_folder(&folder, session);
        }
        let _ = self.owner.update(cx, |o, cx| {
            o.mono_apply_groups(project.clone(), groups.clone(), cx)
        });
        let store = self.store.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = locked(&store).and_then(|db| db.save_groups(&project, &groups));
            })
            .detach();
    }
    pub(super) fn check_completions(&mut self, cx: &mut Context<Self>) {
        let views = self
            .owner
            .upgrade()
            .map(|o| o.read(cx).mono_session_views(cx))
            .unwrap_or_default();
        let mut ready = Vec::new();
        for mono in &self.roster.monos {
            let groups: HashSet<_> = mono
                .launches
                .iter()
                .filter(|l| l.notify_on_complete && !l.completed && !l.dismissed)
                .map(|l| l.origin_turn)
                .collect();
            for group in groups {
                let source = (group >> 32) as i64;
                if self
                    .chats
                    .values()
                    .chain(self.running.values().map(|r| &r.view))
                    .any(|v| v.read(cx).summary.id == source && v.read(cx).busy)
                {
                    continue;
                }
                let children: Vec<_> = mono
                    .launches
                    .iter()
                    .filter(|l| {
                        l.origin_turn == group
                            && l.notify_on_complete
                            && !l.completed
                            && !l.dismissed
                    })
                    .collect();
                if !children.is_empty()
                    && children.iter().all(|l| {
                        views
                            .iter()
                            .find(|v| v.id == l.session_id)
                            .is_none_or(|v| !v.busy && v.has_messages)
                    })
                    && !self.completion_work.contains(&(mono.id.clone(), group))
                {
                    ready.push((
                        mono.id.clone(),
                        group,
                        children
                            .iter()
                            .map(|l| (l.session_id, l.title.clone()))
                            .collect::<Vec<_>>(),
                    ));
                }
            }
        }
        for (id, group, children) in ready {
            self.completion_work.insert((id.clone(), group));
            let store = self.store.clone();
            let work = cx.background_executor().spawn(async move {
                children
                    .into_iter()
                    .map(|(id, title)| {
                        let rows = locked(&store)
                            .and_then(|db| db.load(id))
                            .map(|t| monocode_core::response::prose(&t.rows))
                            .unwrap_or_else(|_| "Session was removed".into());
                        format!(
                            "Session {id}: {title}\n{}",
                            monocode_core::response::limit(&rows, 12000)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n")
            });
            cx.spawn(async move |this,cx| {
                let report=work.await;
                let _=this.update(cx,|this,cx|{
                    this.completion_work.remove(&(id.clone(),group));
                    if let Some(mono)=this.roster.get_mut(&id){for launch in &mut mono.launches{if launch.origin_turn==group{launch.completed=true;}}}
                    let message=format!("[Delegated work completed]\nAll sessions in this group have stopped. Inspect their results and give an integrated update.\n\n{report}");
                    let source=(group>>32) as i64;
                    if let Some(run)=this.running.get_mut(&source){run.mailbox.push(message);this.save(cx);this.habit_changed(source,cx);}else{this.enqueue(&id,message,Vec::new(),true,cx);this.ensure_chat(&id,cx);}
                });
            }).detach();
        }
    }
}
