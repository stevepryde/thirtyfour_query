use crate::conditions::handle_errors;
use crate::{conditions, ElementPoller, ElementPredicate};
use std::time::{Duration, Instant};
use stringmatch::Needle;
use thirtyfour::error::WebDriverError;
use thirtyfour::prelude::WebDriverResult;
use thirtyfour::support::sleep;
use thirtyfour::WebElement;

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
        ElementWaitCondition::new(self)
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
                    sleep(minimum_elapsed - actual_elapsed).await;
                }
            }
        }
    }
}

pub struct ElementWaitCondition<'a> {
    waiter: ElementWaiter<'a>,
    ignore_errors: bool,
}

impl<'a> ElementWaitCondition<'a> {
    pub fn new(waiter: ElementWaiter<'a>) -> Self {
        Self {
            waiter,
            ignore_errors: true,
        }
    }

    fn timeout(self) -> WebDriverResult<()> {
        Err(WebDriverError::Timeout(self.waiter.message))
    }

    /// By default a waiter will return early if any error is returned from thirtyfour.
    /// However, this behaviour can be turned off so that the waiter will continue polling
    /// until it either gets a positive result or reaches the timeout.
    pub fn ignore_errors(mut self, ignore: bool) -> Self {
        self.ignore_errors = ignore;
        self
    }

    pub async fn stale(self) -> WebDriverResult<()> {
        let ignore_errors = self.ignore_errors;

        match self
            .waiter
            .run_poller(Box::new(move |elem| {
                Box::pin(async move {
                    handle_errors(elem.is_present().await.map(|x| !x), ignore_errors)
                })
            }))
            .await?
        {
            true => Ok(()),
            false => self.timeout(),
        }
    }

    pub async fn displayed(self) -> WebDriverResult<()> {
        match self.waiter.run_poller(conditions::element_is_displayed(self.ignore_errors)).await? {
            true => Ok(()),
            false => self.timeout(),
        }
    }

    pub async fn not_displayed(self) -> WebDriverResult<()> {
        match self
            .waiter
            .run_poller(conditions::element_is_not_displayed(self.ignore_errors))
            .await?
        {
            true => Ok(()),
            false => self.timeout(),
        }
    }

    pub async fn selected(self) -> WebDriverResult<()> {
        match self.waiter.run_poller(conditions::element_is_selected(self.ignore_errors)).await? {
            true => Ok(()),
            false => self.timeout(),
        }
    }

    pub async fn not_selected(self) -> WebDriverResult<()> {
        match self
            .waiter
            .run_poller(conditions::element_is_not_selected(self.ignore_errors))
            .await?
        {
            true => Ok(()),
            false => self.timeout(),
        }
    }

    pub async fn enabled(self) -> WebDriverResult<()> {
        match self.waiter.run_poller(conditions::element_is_enabled(self.ignore_errors)).await? {
            true => Ok(()),
            false => self.timeout(),
        }
    }

    pub async fn not_enabled(self) -> WebDriverResult<()> {
        match self.waiter.run_poller(conditions::element_is_not_enabled(self.ignore_errors)).await?
        {
            true => Ok(()),
            false => self.timeout(),
        }
    }

    pub async fn clickable(self) -> WebDriverResult<()> {
        match self.waiter.run_poller(conditions::element_is_clickable(self.ignore_errors)).await? {
            true => Ok(()),
            false => self.timeout(),
        }
    }

    pub async fn not_clickable(self) -> WebDriverResult<()> {
        match self
            .waiter
            .run_poller(conditions::element_is_not_clickable(self.ignore_errors))
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

    pub async fn has_attribute<S, N>(self, attribute_name: S, value: N) -> WebDriverResult<()>
    where
        S: Into<String>,
        N: Needle + Clone + Send + Sync + 'static,
    {
        let ignore_errors = self.ignore_errors;
        self.condition(conditions::element_has_attribute(attribute_name, value, ignore_errors))
            .await
    }

    pub async fn has_not_attribute<S, N>(self, attribute_name: S, value: N) -> WebDriverResult<()>
    where
        S: Into<String>,
        N: Needle + Clone + Send + Sync + 'static,
    {
        let ignore_errors = self.ignore_errors;
        self.condition(conditions::element_has_not_attribute(attribute_name, value, ignore_errors))
            .await
    }

    pub async fn has_attributes<S, N>(self, desired_attributes: &[(S, N)]) -> WebDriverResult<()>
    where
        S: Into<String> + Clone,
        N: Needle + Clone + Send + Sync + 'static,
    {
        let ignore_errors = self.ignore_errors;
        self.condition(conditions::element_has_attributes(desired_attributes, ignore_errors)).await
    }

    pub async fn has_not_attributes<S, N>(
        self,
        desired_attributes: &[(S, N)],
    ) -> WebDriverResult<()>
    where
        S: Into<String> + Clone,
        N: Needle + Clone + Send + Sync + 'static,
    {
        let ignore_errors = self.ignore_errors;
        self.condition(conditions::element_has_not_attributes(desired_attributes, ignore_errors))
            .await
    }

    pub async fn has_property<S, N>(self, property_name: S, value: N) -> WebDriverResult<()>
    where
        S: Into<String>,
        N: Needle + Clone + Send + Sync + 'static,
    {
        let ignore_errors = self.ignore_errors;
        self.condition(conditions::element_has_property(property_name, value, ignore_errors)).await
    }

    pub async fn has_not_property<S, N>(self, property_name: S, value: N) -> WebDriverResult<()>
    where
        S: Into<String>,
        N: Needle + Clone + Send + Sync + 'static,
    {
        let ignore_errors = self.ignore_errors;
        self.condition(conditions::element_has_not_property(property_name, value, ignore_errors))
            .await
    }

    pub async fn has_properties<S, N>(self, desired_properties: &[(S, N)]) -> WebDriverResult<()>
    where
        S: Into<String> + Clone,
        N: Needle + Clone + Send + Sync + 'static,
    {
        let ignore_errors = self.ignore_errors;
        self.condition(conditions::element_has_properties(desired_properties, ignore_errors)).await
    }

    pub async fn has_not_properties<S, N>(
        self,
        desired_properties: &[(S, N)],
    ) -> WebDriverResult<()>
    where
        S: Into<String> + Clone,
        N: Needle + Clone + Send + Sync + 'static,
    {
        let ignore_errors = self.ignore_errors;
        self.condition(conditions::element_has_not_properties(desired_properties, ignore_errors))
            .await
    }

    pub async fn has_css_property<S, N>(self, css_property_name: S, value: N) -> WebDriverResult<()>
    where
        S: Into<String>,
        N: Needle + Clone + Send + Sync + 'static,
    {
        let ignore_errors = self.ignore_errors;
        self.condition(conditions::element_has_css_property(
            css_property_name,
            value,
            ignore_errors,
        ))
        .await
    }

    pub async fn has_not_css_property<S, N>(
        self,
        css_property_name: S,
        value: N,
    ) -> WebDriverResult<()>
    where
        S: Into<String>,
        N: Needle + Clone + Send + Sync + 'static,
    {
        let ignore_errors = self.ignore_errors;
        self.condition(conditions::element_has_not_css_property(
            css_property_name,
            value,
            ignore_errors,
        ))
        .await
    }

    pub async fn has_css_properties<S, N>(
        self,
        desired_css_properties: &[(S, N)],
    ) -> WebDriverResult<()>
    where
        S: Into<String> + Clone,
        N: Needle + Clone + Send + Sync + 'static,
    {
        let ignore_errors = self.ignore_errors;
        self.condition(conditions::element_has_css_properties(
            desired_css_properties,
            ignore_errors,
        ))
        .await
    }

    pub async fn has_not_css_properties<S, N>(
        self,
        desired_css_properties: &[(S, N)],
    ) -> WebDriverResult<()>
    where
        S: Into<String> + Clone,
        N: Needle + Clone + Send + Sync + 'static,
    {
        let ignore_errors = self.ignore_errors;
        self.condition(conditions::element_has_not_css_properties(
            desired_css_properties,
            ignore_errors,
        ))
        .await
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
    let elem = driver.find_element(By::Css(r#"div"#)).await?;

    // ElementWaitCondition
    is_send_val(&elem.wait("Some error").until().stale());
    is_send_val(&elem.wait("Some error").until().displayed());
    is_send_val(&elem.wait("Some error").until().selected());
    is_send_val(&elem.wait("Some error").until().enabled());
    is_send_val(&elem.wait("Some error").until().condition(Box::new(|elem| {
        Box::pin(async move { elem.is_enabled().await.or(Ok(false)) })
    })));

    Ok(())
}
