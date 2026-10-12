//! Chapter 03 §7 — span, event, subscriber — and the `Span::enter` trap in async code.
//!
//! [`Recorder`] is a hand-written `tracing_subscriber::Layer`: it is told when a span is
//! created, entered and exited and when an event fires, and writes each notification as a text
//! line. [`transcript`] runs two tasks on a current-thread runtime in one of three styles and
//! returns what the layer was told. Nothing in the transcript depends on timing, so it is
//! deterministic and the tests can assert on it.

use parking_lot::Mutex;
use std::fmt::Debug;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use tracing::field::{Field, Visit};
use tracing::{Event, Instrument, Subscriber, span};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{Layer, Registry};

/// How the two tasks open their span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// `let _guard = span.enter();` kept alive across `.await`: the documented mistake.
    EnterAcrossAwait,
    /// `async { ... }.instrument(span)`: the span is entered on every poll and exited when the future yields.
    Instrument,
    /// `#[tracing::instrument]` on the `async fn`.
    InstrumentAttribute,
}

impl Style {
    pub fn name(self) -> &'static str {
        match self {
            Style::EnterAcrossAwait => "enter-across-await",
            Style::Instrument => "instrument",
            Style::InstrumentAttribute => "instrument-attribute",
        }
    }

    pub fn parse(s: &str) -> Option<Style> {
        [Style::EnterAcrossAwait, Style::Instrument, Style::InstrumentAttribute].into_iter().find(|v| v.name() == s)
    }
}

/// The label of a span, stored in the span's extensions: `name{task=A}`.
struct Label(String);

#[derive(Default)]
struct FieldText {
    task: Option<String>,
    message: Option<String>,
}

impl Visit for FieldText {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "task" {
            self.task = Some(value.to_owned());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        match field.name() {
            "message" => self.message = Some(format!("{value:?}")),
            "task" => self.task = Some(format!("{value:?}")),
            _ => {}
        }
    }
}

/// A `Layer` that records every notification as one line of text.
#[derive(Clone, Default)]
pub struct Recorder {
    lines: Arc<Mutex<Vec<String>>>,
}

impl Recorder {
    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().clone()
    }

    fn push(&self, line: String) {
        self.lines.lock().push(line);
    }
}

fn label_of<S>(ctx: &Context<'_, S>, id: &span::Id) -> String
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    ctx.span(id)
        .and_then(|span| span.extensions().get::<Label>().map(|l| l.0.clone()))
        .unwrap_or_else(|| "?".into())
}

impl<S> Layer<S> for Recorder
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        let mut fields = FieldText::default();
        attrs.record(&mut fields);
        let name = attrs.metadata().name();
        let label = match fields.task {
            Some(task) => format!("{name}{{task={task}}}"),
            None => name.to_string(),
        };
        let span = ctx.span(id).expect("a new span is in the registry");
        let parent = span.parent().and_then(|p| p.extensions().get::<Label>().map(|l| l.0.clone()));
        span.extensions_mut().insert(Label(label.clone()));
        self.push(format!("new    {label}  (parent: {})", parent.as_deref().unwrap_or("none")));
    }

    fn on_enter(&self, id: &span::Id, ctx: Context<'_, S>) {
        self.push(format!("enter  {}", label_of(&ctx, id)));
    }

    fn on_exit(&self, id: &span::Id, ctx: Context<'_, S>) {
        self.push(format!("exit   {}", label_of(&ctx, id)));
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut fields = FieldText::default();
        event.record(&mut fields);
        let path = ctx
            .event_scope(event)
            .map(|scope| scope.from_root().map(|s| s.extensions().get::<Label>().map_or_else(|| "?".into(), |l| l.0.clone())).collect::<Vec<_>>().join(" > "))
            .unwrap_or_default();
        let message = fields.message.unwrap_or_default();
        self.push(format!("event  {message:<7} reported in [{path}]"));
    }
}

/// Returns `Pending` exactly once and asks to be polled again at once.
///
/// `tokio::task::yield_now` also yields once, but it *defers* the wake-up until the scheduler is
/// otherwise idle, which changes the order in which the tasks resume. Waking at once keeps the
/// local queue first-in-first-out, so the two tasks run A, B, A, B and the transcript reads naturally.
fn yield_once() -> impl Future<Output = ()> {
    struct YieldOnce(bool);
    impl Future for YieldOnce {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<()> {
            if self.0 {
                Poll::Ready(())
            } else {
                self.0 = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
    YieldOnce(false)
}

async fn enter_across_await(task: &'static str) {
    let span = tracing::info_span!("handle", task);
    let _guard = span.enter(); // never dropped at the `.await` below
    tracing::info!("start");
    yield_once().await;
    tracing::info!("resume");
}

async fn with_instrument(task: &'static str) {
    async {
        tracing::info!("start");
        yield_once().await;
        tracing::info!("resume");
    }
    .instrument(tracing::info_span!("handle", task))
    .await
}

#[tracing::instrument(skip_all, fields(task = task))]
async fn with_attribute(task: &'static str) {
    tracing::info!("start");
    yield_once().await;
    tracing::info!("resume");
}

/// Runs task `A` and task `B` on a current-thread runtime, each yielding once in the middle
/// (so the schedule is A, B, A, B), and returns what the [`Recorder`] layer saw.
pub fn transcript(style: Style) -> Vec<String> {
    let recorder = Recorder::default();
    let subscriber = Registry::default().with(recorder.clone());
    let runtime = tokio::runtime::Builder::new_current_thread().build().expect("build current-thread runtime");
    tracing::subscriber::with_default(subscriber, || {
        let local = tokio::task::LocalSet::new();
        for task in ["A", "B"] {
            match style {
                Style::EnterAcrossAwait => local.spawn_local(enter_across_await(task)),
                Style::Instrument => local.spawn_local(with_instrument(task)),
                Style::InstrumentAttribute => local.spawn_local(with_attribute(task)),
            };
        }
        runtime.block_on(local);
    });
    recorder.lines()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The path an event was reported in, for the first event whose message is `message` and
    /// whose index among such events is `nth`.
    fn path_of(lines: &[String], message: &str, nth: usize) -> String {
        let prefix = format!("event  {message:<7} reported in [");
        let found: Vec<&String> = lines.iter().filter(|l| l.starts_with(&prefix)).collect();
        let line = found.get(nth).unwrap_or_else(|| panic!("no event #{nth} named {message} in {lines:#?}"));
        line[prefix.len()..line.len() - 1].to_string()
    }

    fn count(lines: &[String], kind: &str) -> usize {
        lines.iter().filter(|l| l.starts_with(kind)).count()
    }

    /// The trap of chapter 03 §7. With the guard held across `.await`, task B starts while task A's
    /// span is still entered, so B's span is created as A's child, B's `start` event is reported
    /// inside A's span, and A's own `resume` event is reported inside B's span. The whole
    /// transcript is pinned: it is deterministic (a current-thread runtime, no timing).
    #[test]
    fn enter_across_await_leaks_one_tasks_span_into_the_other() {
        let expected = [
            "new    handle{task=A}  (parent: none)",
            "enter  handle{task=A}",
            "event  start   reported in [handle{task=A}]",
            "new    handle{task=B}  (parent: handle{task=A})",
            "enter  handle{task=B}",
            "event  start   reported in [handle{task=A} > handle{task=B}]",
            "event  resume  reported in [handle{task=A} > handle{task=B}]",
            "exit   handle{task=A}",
            "event  resume  reported in [handle{task=A} > handle{task=B}]",
            "exit   handle{task=B}",
        ];
        assert_eq!(transcript(Style::EnterAcrossAwait), expected);
    }

    /// `.instrument(span)` enters the span on every poll and exits when the future yields, so each
    /// event is reported only inside its own task's span and no span is created inside another.
    #[test]
    fn instrument_keeps_each_event_in_its_own_span() {
        let lines = transcript(Style::Instrument);
        for (message, nth, task) in [("start", 0, "A"), ("start", 1, "B"), ("resume", 0, "A"), ("resume", 1, "B")] {
            assert_eq!(path_of(&lines, message, nth), format!("handle{{task={task}}}"), "{lines:#?}");
        }
        assert!(lines.iter().all(|l| !l.contains("(parent: handle")), "no span may be created inside another: {lines:#?}");
        // Three enter/exit pairs per task: the two polls, plus one when the instrumented future is
        // dropped (tracing enters the span so the inner future's drop is attributed to it).
        assert_eq!((count(&lines, "enter"), count(&lines, "exit")), (6, 6));
    }

    /// `#[instrument]` on an `async fn` behaves like `.instrument(...)`.
    #[test]
    fn instrument_attribute_is_equivalent() {
        let lines = transcript(Style::InstrumentAttribute);
        assert_eq!(path_of(&lines, "start", 0), "with_attribute{task=A}");
        assert_eq!(path_of(&lines, "start", 1), "with_attribute{task=B}");
        assert_eq!(path_of(&lines, "resume", 0), "with_attribute{task=A}");
        assert_eq!(path_of(&lines, "resume", 1), "with_attribute{task=B}");
        assert!(lines.iter().all(|l| !l.contains("(parent: with_attribute")), "{lines:#?}");
        assert_eq!((count(&lines, "enter"), count(&lines, "exit")), (6, 6));
    }

    #[test]
    fn style_names_round_trip() {
        for s in [Style::EnterAcrossAwait, Style::Instrument, Style::InstrumentAttribute] {
            assert_eq!(Style::parse(s.name()), Some(s));
        }
        assert_eq!(Style::parse("nope"), None);
    }
}
