#![doc = include_str!("../README.md")]

#[cfg(feature = "util")]
pub mod util;

use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc;
use futures::{stream::FuturesUnordered, StreamExt};
use influxdb::InfluxDbWriteable;

/// Dispatch a single metric to the database.
pub async fn dispatch<M: InfluxDbWriteable>(client: &influxdb::Client, metric: M) {
    let type_name = std::any::type_name::<M>();

    let name = type_name
        .rsplit_once("::")
        .map(|(_, name)| name)
        .unwrap_or(type_name);

    if let Err(error) = client.query(metric.into_query(name)).await {
        tracing::error!("Failed to submit metric: {}", error);
    }
}

/// Dispatch many metrics to the database.
/// These will be dispatched concurrently.
pub async fn dispatch_many<M, I>(client: &influxdb::Client, metrics: I)
where
    M: InfluxDbWriteable,
    I: IntoIterator<Item = M>,
{
    metrics
        .into_iter()
        .map(|metric| dispatch(client, metric))
        .collect::<FuturesUnordered<_>>()
        .collect::<()>()
        .await;
}

/// Aggregator for metrics.
/// An aggregator should collect metrics so they can be batch dispatched.
#[async_trait]
pub trait MetricsConsumer {
    /// The metrics type.
    type Metric;

    /// Create a new instance for the given client.
    fn new(client: influxdb::Client) -> Self;

    /// Consume a metric.
    fn accept(&mut self, metric: Self::Metric);

    /// Flush all consumed metrics to the database.
    async fn flush(&mut self);
}

/// A handle to the InfluxDb metrics recorder.
/// Aborts the submission task when dropped.
#[derive(Debug)]
pub struct InfluxDbHandle<M> {
    /// The channel for submitting metrics.
    channel: mpsc::Sender<M>,
    /// The metrics task, which consumes the metrics in the channel and submits them in an
    /// infinite loop.
    metrics_task: tokio::task::JoinHandle<()>,
}

impl<M> Drop for InfluxDbHandle<M> {
    fn drop(&mut self) {
        self.metrics_task.abort(); // Prevent the task from leaking.
    }
}

impl<M> InfluxDbHandle<M>
where
    M: Send + 'static,
{
    /// Start the metrics task.
    /// This task will run indefinitely, but will be aborted when the handle is dropped.
    pub fn new<C>(consumer: C, push_interval: u64) -> Self
    where
        C: MetricsConsumer<Metric = M> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel(8192);

        let task = Self::push_loop(consumer, rx, push_interval);

        Self {
            channel: tx,
            metrics_task: tokio::task::spawn(task),
        }
    }

    /// Submit a metric.
    /// There is no strong guarantee that the metric will be recorded. It may actually be
    /// discarded if we're struggling to dispatch all metrics.
    pub fn submit(&self, metric: M) {
        if let Err(error) = self.channel.try_send(metric) {
            tracing::error!("Failed to submit metric: {}", error);
        }
    }

    /// InfluxDb push loop.
    /// This function will run indefinitely, so it must be placed inside a task so that it can
    /// be aborted when we're done.
    #[tracing::instrument(skip(consumer, channel))]
    async fn push_loop<C>(mut consumer: C, mut channel: mpsc::Receiver<M>, push_interval: u64)
    where
        C: MetricsConsumer<Metric = M>,
    {
        let mut interval = tokio::time::interval(Duration::from_secs(push_interval));

        tracing::info!("Starting InfluxDb metrics loop");

        loop {
            tokio::select! {
                result = channel.recv() => match result {
                    None => break, // Channel is closed, abort metrics task.
                    Some(metric) => consumer.accept(metric),
                },

                _ = interval.tick() => consumer.flush().await,
            }
        }
    }
}