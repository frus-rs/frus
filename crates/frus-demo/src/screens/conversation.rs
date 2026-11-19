// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! A conversation (milestone 815): a few hundred messages over three days, a reply that types
//! back after each message sent, a burst of messages to scroll up against, and a message that
//! fails until it is retried.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use frus_widgets::host;
use frus_widgets::{ChatComposer, ChatList, ChatListState, ChatMessage, MessageStatus, Text};

use crate::prelude::*;

/// What the other side says, in turn.
const REPLIES: [&str; 5] = [
    "Sounds good.",
    "I'll look at it after lunch — the second half of the list still needs a pass, and the \
     wording on the last screen is not settled yet.",
    "👍",
    "Can you send the numbers again?",
    "Done.",
];

/// Now, as the conversation's local seconds (UTC: the demo does not know the offset).
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The screen.
pub(crate) struct ConversationPage;

/// What the screen keeps.
pub(crate) struct ConversationState {
    messages: Rc<Vec<ChatMessage>>,
    list: ChatListState,
    draft: String,
    typing: bool,
    next_id: u64,
    replies: usize,
}

impl StatefulWidget for ConversationPage {
    type State = ConversationState;

    fn create_state(&self) -> ConversationState {
        let start = now() - 2 * 86_400;
        let mut messages = Vec::new();
        for i in 0..300i64 {
            let time = start + i * 600;
            let message = if i % 3 == 2 {
                ChatMessage::outgoing(format!("h{i}"), format!("Earlier reply {i}"), time)
                    .status(MessageStatus::Read)
            } else {
                ChatMessage::incoming(
                    format!("h{i}"),
                    "ada",
                    format!("Message {i} from the history"),
                    time,
                )
                .author_name("Ada Lovelace")
            };
            messages.push(message);
        }
        messages.push(
            ChatMessage::outgoing("failed", "This one did not go through.", now() - 60)
                .status(MessageStatus::Failed),
        );
        ConversationState {
            messages: Rc::new(messages),
            list: ChatListState::new(),
            draft: String::new(),
            typing: false,
            next_id: 0,
            replies: 0,
        }
    }
}

impl ConversationState {
    fn push(&mut self, message: ChatMessage) {
        Rc::make_mut(&mut self.messages).push(message);
    }

    fn id(&mut self) -> String {
        self.next_id += 1;
        format!("n{}", self.next_id)
    }
}

/// The other side types for a moment, then answers.
fn answer_later(handle: frus_widgets::StateHandle<ConversationState>) {
    handle.set_state(|s| s.typing = true);
    host::after(Duration::from_millis(1400), move || {
        handle.set_state(|s| {
            let words = REPLIES[s.replies % REPLIES.len()];
            s.replies += 1;
            s.typing = false;
            let id = s.id();
            s.push(ChatMessage::incoming(id, "ada", words, now()).author_name("Ada Lovelace"));
        })
    });
}

impl State for ConversationState {
    type Widget = ConversationPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let handle = cx.handle();

        let send = {
            let handle = handle.clone();
            on(move || {
                let mut sent = false;
                handle.set_state(|s| {
                    let text = s.draft.trim().to_string();
                    if text.is_empty() {
                        return;
                    }
                    let id = s.id();
                    s.push(ChatMessage::outgoing(id, text, now()).status(MessageStatus::Delivered));
                    s.draft.clear();
                    // One's own message brings the list back down.
                    s.list = s.list.jump_to_newest();
                    sent = true;
                });
                if sent {
                    answer_later(handle.clone());
                }
            })
        };

        let burst = {
            let handle = handle.clone();
            on(move || {
                handle.set_state(|s| {
                    for k in 0..5 {
                        let id = s.id();
                        s.push(
                            ChatMessage::incoming(id, "bob", format!("Burst message {k}"), now())
                                .author_name("Bob"),
                        );
                    }
                })
            })
        };

        let list = ChatList::new(
            self.messages.clone(),
            &self.list,
            cx.handler(|s, state: ChatListState| s.list = state),
        )
        .typing(if self.typing { vec!["Ada"] } else { vec![] })
        .on_retry(cx.handler(|s, id: String| {
            if let Some(m) = Rc::make_mut(&mut s.messages)
                .iter_mut()
                .find(|m| m.id == id)
            {
                m.status = MessageStatus::Sent;
            }
        }));

        let composer = ChatComposer::new(&self.draft, cx.handler(|s, text: String| s.draft = text))
            .on_send(send)
            .attachment(
                IconButton::new(Icons::ATTACH_FILE)
                    .label("Attach a file")
                    .on_press(on(|| {})),
            );

        let router = cx.router();
        Scaffold::new()
            .background(theme.background)
            .app_bar(
                AppBar::new()
                    .title(Text::new("Conversation"))
                    .leading(IconButton::new(Icons::ARROW_BACK).on_press(on(move || {
                        router.pop();
                    })))
                    .action(
                        IconButton::new(Icons::ADD_COMMENT)
                            .label("Receive five messages")
                            .on_press(burst),
                    ),
            )
            .body(
                Flex::column()
                    .child(Expanded::new(list))
                    .child(Divider::new())
                    .child(SafeArea::new(composer)),
            )
            .build()
    }
}
