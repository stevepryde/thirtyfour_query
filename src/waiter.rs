use crate::{ElementPoller, ElementPredicate};
use std::time::Duration;
use thirtyfour::error::WebDriverError;
use thirtyfour::prelude::WebDriverResult;
use thirtyfour::WebElement;
use tokio::time::{delay_for, Instant};

pub struct ElementWaiter<'a> {
    element: &'a WebElement<'a>,
    poller: ElementPoller,
    inverted: bool,
    message: String,
}

impl<'a> ElementWaiter<'a> {
    fn new<S>(element: &'a WebElement<'a>, poller: ElementPoller, message: S) -> Self
    where
        S: Into<String>,
    {
        Self {
            element,
            poller,
            inverted: false,
            message: message.into(),
        }
    }

    /// Use the specified ElementPoller for this ElementWaiter.
    /// This will not affect the default ElementPoller used for other waits.
    pub fn with_poller(mut self, poller: ElementPoller) -> Self {
        self.poller = poller;
        self
    }

    /// Force this ElementWaiter to wait for the specified timeout, polling once
    /// after each interval. This will override the poller for this
    /// ElementWaiter only.
    pub fn wait(self, timeout: Duration, interval: Duration) -> Self {
        self.with_poller(ElementPoller::TimeoutWithInterval(timeout, interval))
    }

    pub fn until(self) -> ElementWaitCondition<'a> {
        ElementWaitCondition {
            waiter: self,
        }
    }

    pub fn until_not(mut self) -> ElementWaitCondition<'a> {
        self.inverted = true;
        ElementWaitCondition {
            waiter: self,
        }
    }

    fn check(&self, value: bool) -> bool {
        if self.inverted {
            !value
        } else {
            value
        }
    }

    async fn run_poller(&self, f: ElementPredicate) -> WebDriverResult<bool> {
        match self.poller {
            ElementPoller::NoWait => self.run_poller_with_options(None, None, 0, f).await,
            ElementPoller::TimeoutWithInterval(timeout, interval) => {
                self.run_poller_with_options(Some(timeout), Some(interval), 0, f).await
            }
            ElementPoller::NumTriesWithInterval(max_tries, interval) => {
                self.run_poller_with_options(None, Some(interval), max_tries, f).await
            }
            ElementPoller::TimeoutWithIntervalAndMinTries(timeout, interval, min_tries) => {
                self.run_poller_with_options(Some(timeout), Some(interval), min_tries, f).await
            }
        }
    }

    async fn run_poller_with_options(
        &self,
        timeout: Option<Duration>,
        interval: Option<Duration>,
        min_tries: u32,
        f: ElementPredicate,
    ) -> WebDriverResult<bool> {
        let mut tries = 0;
        let start = Instant::now();
        loop {
            tries += 1;

            if self.check(f(&self.element).await?) {
                return Ok(true);
            }

            if timeout.is_none() && tries >= min_tries {
                return Ok(false);
            }

            if let Some(i) = interval {
                // Next poll is due no earlier than this long after the first poll started.
                let minimum_elapsed = i * tries;

                // But this much time has elapsed since the first poll started.
                let actual_elapsed = start.elapsed();

                if actual_elapsed < minimum_elapsed {
                    // So we need to wait this much longer.
                    delay_for(minimum_elapsed - actual_elapsed).await;
                }
            }
        }
    }
}

pub struct ElementWaitCondition<'a> {
    waiter: ElementWaiter<'a>,
}

impl<'a> ElementWaitCondition<'a> {
    fn timeout(self) -> WebDriverResult<()> {
        Err(WebDriverError::Timeout(self.waiter.message))
    }

    pub async fn stale(self) -> WebDriverResult<()> {
        match self
            .waiter
            .run_poller(Box::new(|elem| {
                Box::pin(async move { elem.is_present().await.map(|x| !x) })
            }))
            .await?
        {
            true => Ok(()),
            false => self.timeout(),
        }
    }

    pub async fn displayed(self) -> WebDriverResult<()> {
        match self
            .waiter
            .run_poller(Box::new(|elem| Box::pin(async move { elem.is_displayed().await })))
            .await?
        {
            true => Ok(()),
            false => self.timeout(),
        }
    }

    pub async fn selected(self) -> WebDriverResult<()> {
        match self
            .waiter
            .run_poller(Box::new(|elem| Box::pin(async move { elem.is_selected().await })))
            .await?
        {
            true => Ok(()),
            false => self.timeout(),
        }
    }

    pub async fn enabled(self) -> WebDriverResult<()> {
        match self
            .waiter
            .run_poller(Box::new(|elem| Box::pin(async move { elem.is_enabled().await })))
            .await?
        {
            true => Ok(()),
            false => self.timeout(),
        }
    }

    pub async fn condition(self, f: ElementPredicate) -> WebDriverResult<()> {
        match self.waiter.run_poller(f).await? {
            true => Ok(()),
            false => self.timeout(),
        }
    }
}

/// Trait for enabling the ElementWaiter interface.
pub trait ElementWaitable {
    fn wait<S>(&self, message: S) -> ElementWaiter
    where
        S: Into<String>;
}

impl ElementWaitable for WebElement<'_> {
    /// Return an ElementQuery instance for more executing powerful element queries.
    fn wait<S>(&self, message: S) -> ElementWaiter
    where
        S: Into<String>,
    {
        let poller: ElementPoller =
            self.session.config().get("ElementPoller").unwrap_or(ElementPoller::NoWait);
        ElementWaiter::new(&self, poller, message)
    }
}

#[cfg(test)]
/// This function checks if the public async methods implement Send. It is not intended to be executed.
async fn _test_is_send() -> WebDriverResult<()> {
    use thirtyfour::prelude::*;

    // Helper methods
    fn is_send<T: Send>() {}
    fn is_send_val<T: Send>(_val: &T) {}

    // Pre values
    let caps = DesiredCapabilities::chrome();
    let driver = WebDriver::new("http://localhost:4444", &caps).await?;

    // ElementWaitCondition
    is_send_val(&driver.wait("Some error").until().stale());
    is_send_val(&driver.wait("Some error").until().displayed());
    is_send_val(&driver.wait("Some error").until().selected());
    is_send_val(&driver.wait("Some error").until().enabled());
    is_send_val(&driver.wait("Some error").until().condition(Box::new(|_el| async move { true })));

    Ok(())
}
