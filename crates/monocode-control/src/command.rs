//! The registry: one [`Command`] variant per operation, generated from one
//! list, so adding an operation is one line here plus its types under `ops`.
use crate::{Descriptor, Hints, Operation, Scope, ops};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An operation that is in the registry, so its input can be wrapped in a
/// [`Command`]. Implemented by the registry macro only.
pub trait Registered: Operation {
    /// Wraps `input` in this operation's command.
    fn command(input: Self::Input) -> Command;
}

macro_rules! registry {
    ($($variant:ident => $op:ty),+ $(,)?) => {
        /// One call: the operation's name as `operation` and its typed
        /// arguments as `input`. Variant names in `snake_case` are the
        /// operations' [`Operation::NAME`]s; a test holds them equal.
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(tag = "operation", content = "input", rename_all = "snake_case")]
        pub enum Command {
            $(
                #[doc = concat!("See [`", stringify!($op), "`].")]
                $variant(<$op as Operation>::Input),
            )+
        }

        impl Command {
            /// The operation's tool name.
            #[must_use]
            pub fn name(&self) -> &'static str {
                match self {
                    $(Self::$variant(_) => <$op as Operation>::NAME,)+
                }
            }
            /// The scope the operation needs.
            #[must_use]
            pub fn scope(&self) -> Scope {
                match self {
                    $(Self::$variant(_) => <$op as Operation>::SCOPE,)+
                }
            }
            /// Every scope this call needs, given its input: the
            /// operation's, then its [`Operation::ALSO`] when the input
            /// asks for them ([`Operation::needs_also`]).
            #[must_use]
            pub fn scopes(&self) -> Vec<Scope> {
                match self {
                    $(Self::$variant(input) => {
                        let mut scopes = vec![<$op as Operation>::SCOPE];
                        if <$op as Operation>::needs_also(input) {
                            scopes.extend_from_slice(<$op as Operation>::ALSO);
                        }
                        scopes
                    })+
                }
            }
            /// How the operation behaves.
            #[must_use]
            pub fn hints(&self) -> Hints {
                match self {
                    $(Self::$variant(_) => <$op as Operation>::HINTS,)+
                }
            }
            /// Whether the person confirms the call in the app first
            /// ([`Operation::CONFIRM`]).
            #[must_use]
            pub fn confirmed(&self) -> bool {
                match self {
                    $(Self::$variant(_) => <$op as Operation>::CONFIRM,)+
                }
            }
            /// The existing session the call acts on, if it names one
            /// ([`Operation::target_session`]).
            #[must_use]
            pub fn target_session(&self) -> Option<i64> {
                match self {
                    $(Self::$variant(input) => <$op as Operation>::target_session(input),)+
                }
            }
            /// Whether a session may not make this call on its own session
            /// ([`Operation::NOT_ON_SELF`]).
            #[must_use]
            pub fn not_on_self(&self) -> bool {
                match self {
                    $(Self::$variant(_) => <$op as Operation>::NOT_ON_SELF,)+
                }
            }
            /// Whether the target session must be local
            /// ([`Operation::LOCAL_ONLY`]).
            #[must_use]
            pub fn local_only(&self) -> bool {
                match self {
                    $(Self::$variant(_) => <$op as Operation>::LOCAL_ONLY,)+
                }
            }
            /// Whether only a Mono's chat or habit run may make this call
            /// ([`Operation::MONO_ONLY`]).
            #[must_use]
            pub fn mono_only(&self) -> bool {
                match self {
                    $(Self::$variant(_) => <$op as Operation>::MONO_ONLY,)+
                }
            }
            /// The call's `project`, when an absent one means the current
            /// project ([`Operation::project_slot`]).
            pub fn project_slot(&mut self) -> Option<&mut Option<String>> {
                match self {
                    $(Self::$variant(input) => <$op as Operation>::project_slot(input),)+
                }
            }
        }

        $(
            impl Registered for $op {
                fn command(input: Self::Input) -> Command {
                    Command::$variant(input)
                }
            }
        )+

        /// Every operation's catalog entry, in registry order.
        #[must_use]
        pub fn catalog() -> Vec<Descriptor> {
            vec![$(Descriptor::of::<$op>(),)+]
        }

        /// The [`Operation::TITLE`] and [`Operation::SUBJECT`] of the operation
        /// named `name`, without building its schemas.
        pub(crate) fn caption_spec(name: &str) -> Option<(&'static str, &'static [&'static str])> {
            $(
                if name == <$op as Operation>::NAME {
                    return Some((<$op as Operation>::TITLE, <$op as Operation>::SUBJECT));
                }
            )+
            None
        }

        /// The [`Operation::HINTS`] of the operation named `name`, without
        /// building its schemas.
        #[must_use]
        pub fn hints_of(name: &str) -> Option<Hints> {
            $(
                if name == <$op as Operation>::NAME {
                    return Some(<$op as Operation>::HINTS);
                }
            )+
            None
        }

        /// A one-line summary of `output` from the operation named `name`, or
        /// `None` when the name is unknown or the output does not match its type.
        #[must_use]
        pub fn summarize(name: &str, output: &Value) -> Option<String> {
            $(
                if name == <$op as Operation>::NAME {
                    return <<$op as Operation>::Output as Deserialize>::deserialize(output)
                        .ok()
                        .map(|output| <$op as Operation>::summary(&output));
                }
            )+
            None
        }
    };
}

registry! {
    AppInfo => ops::app::AppInfo,
    ModelList => ops::model::ModelList,
    ProjectList => ops::project::ProjectList,
    ProjectSelect => ops::project::ProjectSelect,
    ProjectAdd => ops::project::ProjectAdd,
    WorkspaceRead => ops::workspace::WorkspaceRead,
    SessionList => ops::session::SessionList,
    SessionRead => ops::session::SessionRead,
    SessionStatus => ops::session::SessionStatusRead,
    SessionStart => ops::dispatch::SessionStart,
    SessionSend => ops::dispatch::SessionSend,
    SessionWait => ops::dispatch::SessionWait,
    SessionStop => ops::dispatch::SessionStop,
    TabOpen => ops::tab::TabOpen,
    TabFocus => ops::tab::TabFocus,
    TabClose => ops::tab::TabClose,
    SessionConfigure => ops::configure::SessionConfigure,
    SessionDraft => ops::configure::SessionDraft,
    SessionRename => ops::organize::SessionRename,
    SessionArchive => ops::organize::SessionArchive,
    SessionRestore => ops::organize::SessionRestore,
    FolderList => ops::organize::FolderList,
    SessionMove => ops::organize::SessionMove,
    QuestionList => ops::question::QuestionList,
    QuestionAnswer => ops::question::QuestionAnswer,
    QueueList => ops::queue::QueueList,
    QueueRemove => ops::queue::QueueRemove,
    QueueSteer => ops::queue::QueueSteer,
    QueueResume => ops::queue::QueueResume,
    QueueEdit => ops::queue::QueueEdit,
    WorktreeList => ops::worktree::WorktreeList,
    WorktreeCreate => ops::worktree::WorktreeCreate,
    ProjectUpdate => ops::rail::ProjectUpdate,
    ProjectArchive => ops::rail::ProjectArchive,
    ProjectUnarchive => ops::rail::ProjectUnarchive,
    ProjectReorder => ops::rail::ProjectReorder,
    TabMove => ops::layout::TabMove,
    PaneSplit => ops::layout::PaneSplit,
    LayoutSet => ops::layout::LayoutSet,
    SettingsRead => ops::settings::SettingsRead,
    SettingsUpdate => ops::settings::SettingsUpdate,
    SearchSessions => ops::search::SearchSessions,
    SearchProject => ops::search::SearchProject,
    NotesList => ops::notes::NotesList,
    NotesRead => ops::notes::NotesRead,
    NotesWrite => ops::notes::NotesWrite,
    ChangesRead => ops::changes::ChangesRead,
    SessionCompact => ops::lifecycle::SessionCompact,
    SessionHandoff => ops::lifecycle::SessionHandoff,
    SessionDelete => ops::lifecycle::SessionDelete,
    ProjectDelete => ops::lifecycle::ProjectDelete,
    GitStage => ops::vcs::GitStage,
    GitUnstage => ops::vcs::GitUnstage,
    GitCommit => ops::vcs::GitCommit,
    GitDiscard => ops::vcs::GitDiscard,
    GitPush => ops::vcs::GitPush,
    AutomationList => ops::automation::AutomationList,
    AutomationRead => ops::automation::AutomationRead,
    AutomationRun => ops::automation::AutomationRun,
    AutomationSetEnabled => ops::automation::AutomationSetEnabled,
    AutomationUpsert => ops::automation::AutomationUpsert,
    AutomationDelete => ops::automation::AutomationDelete,
    MonoList => ops::mono::MonoList,
    MonoOpen => ops::mono::MonoOpen,
    MonoSend => ops::mono::MonoSend,
    MonoMemoryRead => ops::mono_memory::MonoMemoryRead,
    MonoMemorySearch => ops::mono_memory::MonoMemorySearch,
    MonoMemoryAdd => ops::mono_memory::MonoMemoryAdd,
    MonoMemoryReplace => ops::mono_memory::MonoMemoryReplace,
    MonoMemoryRemove => ops::mono_memory::MonoMemoryRemove,
    MonoSoulRead => ops::mono_soul::MonoSoulRead,
    MonoSoulUpdate => ops::mono_soul::MonoSoulUpdate,
    MonoHabitList => ops::mono_habit::MonoHabitList,
    MonoHabitAdd => ops::mono_habit::MonoHabitAdd,
    MonoHabitUpdate => ops::mono_habit::MonoHabitUpdate,
    MonoHabitRun => ops::mono_habit::MonoHabitRun,
    MonoHabitRemove => ops::mono_habit::MonoHabitRemove,
    MonoArtifactList => ops::mono_artifact::MonoArtifactList,
    MonoArtifactRead => ops::mono_artifact::MonoArtifactRead,
    MonoArtifactWrite => ops::mono_artifact::MonoArtifactWrite,
    MonoCardPost => ops::mono_card::MonoCardPost,
}
