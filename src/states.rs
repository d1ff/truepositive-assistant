use crate::commands::BotCommand;

machine!(
    enum UserState {
        Idle,
        InBacklog { pub top: i32, pub skip: i32 },
    }
);

transitions!(UserState, [
    (Idle, BotCommand) => [InBacklog, Idle],
    (InBacklog, BotCommand) => [InBacklog, Idle]
]);

impl Idle {
    pub fn on_bot_command(&self, cmd: BotCommand) -> UserState {
        match cmd {
            BotCommand::Backlog(_) => UserState::inbacklog(10, 0),
            _ => UserState::idle(),
        }
    }
}

impl InBacklog {
    pub fn on_bot_command(&self, cmd: BotCommand) -> UserState {
        match cmd {
            BotCommand::BacklogStop(_) => UserState::idle(),
            BotCommand::BacklogNext(_, p) | BotCommand::BacklogPrev(_, p) => {
                UserState::inbacklog(p.top, p.skip)
            }
            _ => UserState::inbacklog(self.top, self.skip),
        }
    }
}
