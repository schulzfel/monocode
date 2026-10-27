//! The fields each old app action took, frozen as the old validator listed
//! them, so a field an action never took is still refused.

/// The fields old action `action` took, space-separated, or `None` for an
/// action there never was.
pub(crate) fn fields(action: &str) -> Option<&'static str> {
    Some(match action {
        "models.list" | "soul.read" | "habits.list" => "",
        "sessions.list" | "folders.list" | "worktrees.list" => "project",
        "sessions.read" => "sessionId project before limit maxChars",
        "sessions.start" => {
            "prompt project harness model modelSettings effort runtimeMode title draft reveal workspaceMode worktreeBase worktreeCwd placement besideSessionId notifyOnComplete"
        }
        "sessions.send" => "sessionId prompt project notifyOnComplete",
        "sessions.draft" => "sessionId prompt project",
        "sessions.stop" | "sessions.archive" | "sessions.delete" => "sessionId project",
        "worktrees.create" => "branch base existing project",
        "folders.move" => "sessionId folderId newFolderName project",
        "notes.list" => "limit offset",
        "notes.read" => "id",
        "notes.write" => "id title body tags",
        "artifacts.list" => "kind limit offset",
        "artifacts.read" => "id",
        "artifacts.write" => "id kind title body summary",
        "soul.update" => "text expectedHash",
        "memory.read" => "topic",
        "memory.search" => "query since",
        "memory.add" => "fact topic until",
        "memory.replace" => "find fact topic until",
        "memory.remove" => "find topic",
        "habits.add" => "name instructions schedule",
        "habits.update" => "id name instructions schedule enabled",
        "habits.run" | "habits.remove" => "id",
        "chat.card" => "type repo number note sessionId options name instructions schedule",
        _ => return None,
    })
}
