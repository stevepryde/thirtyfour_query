use tokio::time::{delay_for, Duration, Instant};

use futures::Future;
use serde::{Deserialize, Serialize};
use std::mem;
use std::pin::Pin;
use stringmatch::Needle;
use thirtyfour::error::{WebDriverError, WebDriverErrorInfo};
use thirtyfour::prelude::{WebDriver, WebDriverResult};
use thirtyfour::{By, WebDriverCommands, WebDriverSession, WebElement};

/// Get String containing comma-separated list of selectors used.
fn get_selector_summary(selectors: &[ElementSelector]) -> String {
    let criteria: Vec<String> = selectors.iter().map(|s| s.by.to_string()).collect();
    format!("[{}]", criteria.join(","))
}

/// Helper function to return the NoSuchElement error struct.
fn no_such_element(selectors: &[ElementSelector]) -> WebDriverError {
    WebDriverError::NoSuchElement(WebDriverErrorInfo::new(&format!(
        "Element(s) not found using selectors: {}",
        &get_selector_summary(selectors)
    )))
}

/// Parameters used to determine the polling / timeout behaviour.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ElementPoller {
    /// No polling, single attempt.
    NoWait,
    /// Poll up to the specified timeout, with the specified interval being the
    /// minimum time elapsed between the start of each poll attempt.
    /// If the previous poll attempt took longer than the interval, the next will
    /// start immediately. Once the timeout is reached, a Timeout error will be
    /// returned regardless of the actual number of polling attempts completed.
    TimeoutWithInterval(Duration, Duration),
    /// Poll once every interval, up to the maximum number of polling attempts.
    /// If the previous poll attempt took longer than the interval, the next will
    /// start immediately. However, in the case that the desired element is not
    /// found, you will be guaranteed the specified number of polling attempts,
    /// regardless of how long it takes.
    NumTriesWithInterval(u32, Duration),
    /// Poll once every interval, up to the specified timeout, or the specified
    /// minimum number of polling attempts, whichever comes last.
    /// If the previous poll attempt took longer than the interval, the next will
    /// start immediately. If the timeout was reached before the minimum number
    /// of polling attempts has been executed, then the query will continue
    /// polling until the number of polling attempts equals the specified minimum.
    /// If the minimum number of polling attempts is reached prior to the
    /// specified timeout, then the polling attempts will continue until the
    /// timeout is reached instead.
    TimeoutWithIntervalAndMinTries(Duration, Duration, u32),
}

/// Function signature for element filters.
type ElementFilter =
    Box<dyn for<'a> Fn(&'a WebElement<'a>) -> Pin<Box<dyn Future<Output = bool> + 'a>>>;

/// An ElementSelector contains a selector method (By) as well as zero or more filters.
/// The filters will be applied to any elements matched by the selector.
/// Selectors and filters all run in full on every poll iteration.
pub struct ElementSelector<'a> {
    /// If false (default), find_elements() will be used. If true, find_element() will be used
    /// instead. See notes below for `with_single_selector()` for potential pitfalls.
    pub single: bool,
    pub by: By<'a>,
    pub filters: Vec<ElementFilter>,
}

impl<'a> ElementSelector<'a> {
    pub fn new(by: By<'a>) -> Self {
        Self {
            single: false,
            by: by.clone(),
            filters: Vec::new(),
        }
    }

    /// Call `set_single()` to tell this selector to use find_element() rather than
    /// find_elements(). This can be slightly faster but only really makes sense if
    /// (1) you're not using any filters and (2) you're only interested in the first
    /// element matched anyway.
    pub fn set_single(&mut self) {
        self.single = true;
    }

    /// Add the specified filter to the list of filters for this selector.
    pub fn add_filter(&mut self, f: ElementFilter) {
        self.filters.push(f);
    }

    /// Run all filters for this selector on the specified WebElement vec.
    pub async fn run_filters<'b>(&self, mut elements: Vec<WebElement<'b>>) -> Vec<WebElement<'b>> {
        for func in &self.filters {
            let tmp_elements = mem::replace(&mut elements, Vec::new());
            for element in tmp_elements {
                if func(&element).await {
                    elements.push(element);
                }
            }

            if elements.is_empty() {
                break;
            }
        }

        elements
    }
}

/// Elements can be queried from either a WebDriver or from a WebElement.
/// The command issued to the webdriver will differ depending on the source,
/// i.e. FindElement vs FindElementFromElement etc. but the ElementQuery
/// interface is the same for both.
pub enum ElementQuerySource<'a> {
    Driver(&'a WebDriverSession),
    Element(&'a WebElement<'a>),
}

/// High-level interface for performing powerful element queries using a
/// builder pattern.
///
/// # Example:
/// ```rust
/// # use thirtyfour::prelude::*;
/// # use thirtyfour::support::block_on;
/// # use thirtyfour_query::{ElementPoller, ElementQueryable};
/// # use std::time::Duration;
/// #
/// # fn main() -> WebDriverResult<()> {
/// #     block_on(async {
/// #         let caps = DesiredCapabilities::chrome();
/// #         let mut driver = WebDriver::new("http://localhost:4444/wd/hub", &caps).await?;
/// // Disable implicit timeout in order to use new query interface.
/// driver.set_implicit_wait_timeout(Duration::new(0, 0)).await?;
///
/// let poller = ElementPoller::TimeoutWithInterval(Duration::new(10, 0), Duration::from_millis(500));
/// driver.config_mut().set("ElementPoller", poller)?;
///
/// driver.get("http://webappdemo").await?;
///
/// let elem = driver.query(By::Id("button1")).first().await?;
/// #         assert_eq!(elem.tag_name().await?, "button");
/// #         Ok(())
/// #     })
/// # }
/// ```
pub struct ElementQuery<'a> {
    source: ElementQuerySource<'a>,
    poller: ElementPoller,
    selectors: Vec<ElementSelector<'a>>,
}

impl<'a> ElementQuery<'a> {
    pub fn new(source: ElementQuerySource<'a>, poller: ElementPoller, by: By<'a>) -> Self {
        let selector = ElementSelector::new(by.clone());
        Self {
            source,
            poller,
            selectors: vec![selector],
        }
    }

    /// Use the specified ElementPoller for this ElementQuery.
    /// This will not affect the default ElementPoller used for other queries.
    pub fn with_poller(mut self, poller: ElementPoller) -> Self {
        self.poller = poller;
        self
    }

    /// Force this ElementQuery to wait for the specified timeout, polling once
    /// after each interval. This will override the poller for this
    /// ElementQuery only.
    pub fn wait(self, timeout: Duration, interval: Duration) -> Self {
        self.with_poller(ElementPoller::TimeoutWithInterval(timeout, interval))
    }

    /// Force this ElementQuery to not wait for the specified condition(s).
    /// This will override the poller for this ElementQuery only.
    pub fn nowait(self) -> Self {
        self.with_poller(ElementPoller::NoWait)
    }

    /// Add the specified selector to this ElementQuery. Callers should use
    /// the `or()` method instead.
    fn add_selector(mut self, selector: ElementSelector<'a>) -> Self {
        self.selectors.push(selector);
        self
    }

    /// Add a new selector to this ElementQuery. All conditions specified after
    /// this selector (up until the next `or()` method) will apply to this
    /// selector.
    pub fn or(self, by: By<'a>) -> Self {
        self.add_selector(ElementSelector::new(by))
    }

    /// Return true if an element matches any selector, otherwise false.
    /// This method will not wait, and will not mutate the underlying ElementQuery.
    pub async fn exists(&self) -> WebDriverResult<bool> {
        let elements = self.run_poller_with_options(None, None, 0).await?;
        Ok(!elements.is_empty())
    }

    /// Return only the first WebElement that matches any selector (including all of
    /// the filters for that selector).
    pub async fn first(&self) -> WebDriverResult<WebElement<'a>> {
        let mut elements = self.run_poller().await?;

        if elements.is_empty() {
            Err(no_such_element(&self.selectors))
        } else {
            Ok(elements.remove(0))
        }
    }

    /// Return all WebElements that match any one selector (including all of the
    /// filters for that selector).
    ///
    /// Returns an empty Vec if no elements match.
    pub async fn all(&self) -> WebDriverResult<Vec<WebElement<'a>>> {
        self.run_poller().await
    }

    /// Return all WebElements that match any one selector (including all of the
    /// filters for that selector).
    ///
    /// Returns Err(WebDriverError::NoSuchElement) if no elements match.
    pub async fn all_required(&self) -> WebDriverResult<Vec<WebElement<'a>>> {
        let elements = self.run_poller().await?;

        if elements.is_empty() {
            Err(no_such_element(&self.selectors))
        } else {
            Ok(elements)
        }
    }

    /// Run the poller for this ElementQuery and return the Vec of WebElements matched.
    async fn run_poller(&self) -> WebDriverResult<Vec<WebElement<'a>>> {
        match self.poller {
            ElementPoller::NoWait => self.run_poller_with_options(None, None, 0).await,
            ElementPoller::TimeoutWithInterval(timeout, interval) => {
                self.run_poller_with_options(Some(timeout), Some(interval), 0).await
            }
            ElementPoller::NumTriesWithInterval(max_tries, interval) => {
                self.run_poller_with_options(None, Some(interval), max_tries).await
            }
            ElementPoller::TimeoutWithIntervalAndMinTries(timeout, interval, min_tries) => {
                self.run_poller_with_options(Some(timeout), Some(interval), min_tries).await
            }
        }
    }

    /// Execute the specified selector and return any matched WebElements.
    async fn fetch_elements_from_source(
        &self,
        selector: &ElementSelector<'a>,
    ) -> WebDriverResult<Vec<WebElement<'a>>> {
        let by = selector.by.clone();
        match selector.single {
            true => match self.source {
                ElementQuerySource::Driver(driver) => {
                    driver.find_element(by).await.map(|x| vec![x])
                }
                ElementQuerySource::Element(element) => {
                    element.find_element(by).await.map(|x| vec![x])
                }
            },
            false => match self.source {
                ElementQuerySource::Driver(driver) => driver.find_elements(by).await,
                ElementQuerySource::Element(element) => element.find_elements(by).await,
            },
        }
    }

    /// Run the specified poller with the corresponding timeout, interval
    /// and num_tries parameters.
    async fn run_poller_with_options(
        &self,
        timeout: Option<Duration>,
        interval: Option<Duration>,
        min_tries: u32,
    ) -> WebDriverResult<Vec<WebElement<'a>>> {
        let no_such_element_error = no_such_element(&self.selectors);
        if self.selectors.is_empty() {
            return Err(no_such_element_error);
        }
        let mut tries = 0;

        let start = Instant::now();
        loop {
            tries += 1;

            for selector in &self.selectors {
                let mut elements = match self.fetch_elements_from_source(selector).await {
                    Ok(x) => x,
                    Err(WebDriverError::NoSuchElement(_)) => Vec::new(),
                    Err(e) => return Err(e),
                };

                if !elements.is_empty() {
                    elements = selector.run_filters(elements).await;

                    if !elements.is_empty() {
                        return Ok(elements);
                    }
                }

                if let Some(t) = timeout {
                    if start.elapsed() >= t && tries >= min_tries {
                        return Err(no_such_element_error);
                    }
                }
            }

            if timeout.is_none() && tries >= min_tries {
                return Err(no_such_element_error);
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

    /// Add the specified ElementFilter to the last selector.
    pub fn with_filter(mut self, f: ElementFilter) -> Self {
        if let Some(selector) = self.selectors.last_mut() {
            selector.add_filter(f);
        }
        self
    }

    /// Set the previous selector to only return the first matched element.
    /// WARNING: Use with caution! This can result in faster lookups, but will probably break
    ///          any filters on this selector.
    ///
    /// If you are simply want to get the first element after filtering from a list,
    /// use the `first()` method instead.
    pub fn with_single_selector(mut self) -> Self {
        if let Some(selector) = self.selectors.last_mut() {
            selector.set_single();
        }
        self
    }

    /// Only match elements that are enabled.
    pub fn and_enabled(self) -> Self {
        self.with_filter(Box::new(|elem| {
            Box::pin(async move {
                match elem.is_enabled().await {
                    Ok(x) => x,
                    _ => false,
                }
            })
        }))
    }

    /// Only match elements that are NOT enabled.
    pub fn and_not_enabled(self) -> Self {
        self.with_filter(Box::new(|elem| {
            Box::pin(async move {
                match elem.is_enabled().await {
                    Ok(x) => !x,
                    _ => false,
                }
            })
        }))
    }

    /// Only match elements that are selected.
    pub fn and_selected(self) -> Self {
        self.with_filter(Box::new(|elem| {
            Box::pin(async move {
                match elem.is_selected().await {
                    Ok(x) => x,
                    _ => false,
                }
            })
        }))
    }

    /// Only match elements that are NOT selected.
    pub fn and_not_selected(self) -> Self {
        self.with_filter(Box::new(|elem| {
            Box::pin(async move {
                match elem.is_selected().await {
                    Ok(x) => !x,
                    _ => false,
                }
            })
        }))
    }

    /// Only match elements that have the specified text.
    /// See the `Needle` documentation for more details on text matching rules.
    pub fn with_text<N>(self, text: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        self.with_filter(Box::new(move |elem| {
            let text = text.clone();
            Box::pin(async move {
                match elem.text().await {
                    Ok(x) => text.is_match(&x),
                    _ => false,
                }
            })
        }))
    }

    /// Only match elements that have the specified id.
    /// See the `Needle` documentation for more details on text matching rules.
    pub fn with_id<N>(self, id: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        self.with_filter(Box::new(move |elem| {
            let id = id.clone();
            Box::pin(async move {
                match elem.id().await {
                    Ok(x) => id.is_match(&x),
                    _ => false,
                }
            })
        }))
    }

    /// Only match elements that have the specified class name.
    /// See the `Needle` documentation for more details on text matching rules.
    pub fn with_class<N>(self, class_name: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        self.with_filter(Box::new(move |elem| {
            let class_name = class_name.clone();
            Box::pin(async move {
                match elem.class_name().await {
                    Ok(x) => class_name.is_match(&x),
                    _ => false,
                }
            })
        }))
    }

    /// Only match elements that have the specified tag.
    /// See the `Needle` documentation for more details on text matching rules.
    pub fn with_tag<N>(self, tag_name: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        self.with_filter(Box::new(move |elem| {
            let tag_name = tag_name.clone();
            Box::pin(async move {
                match elem.tag_name().await {
                    Ok(x) => tag_name.is_match(&x),
                    _ => false,
                }
            })
        }))
    }

    /// Only match elements that have the specified value.
    /// See the `Needle` documentation for more details on text matching rules.
    pub fn with_value<N>(self, value: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        self.with_filter(Box::new(move |elem| {
            let value = value.clone();
            Box::pin(async move {
                match elem.value().await {
                    Ok(x) => value.is_match(&x),
                    _ => false,
                }
            })
        }))
    }

    /// Only match elements that have the specified attribute with the specified value.
    /// See the `Needle` documentation for more details on text matching rules.
    pub fn with_attribute<N>(self, attribute_name: &str, value: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        let attribute_name = attribute_name.to_string();
        self.with_filter(Box::new(move |elem| {
            let attribute_name = attribute_name.clone();
            let value = value.clone();
            Box::pin(async move {
                match elem.get_attribute(&attribute_name).await {
                    Ok(x) => value.is_match(&x),
                    _ => false,
                }
            })
        }))
    }

    /// Only match elements that have the specified attributes with the specified values.
    /// See the `Needle` documentation for more details on text matching rules.
    pub fn with_attributes<N>(self, desired_attributes: &'static [(String, N)]) -> Self
    where
        N: Needle + Clone + 'static,
    {
        self.with_filter(Box::new(move |elem| {
            Box::pin(async move {
                for (attribute_name, value) in desired_attributes {
                    match elem.get_attribute(&attribute_name).await {
                        Ok(x) => {
                            if !value.is_match(&x) {
                                return false;
                            }
                        }
                        _ => return false,
                    }
                }
                true
            })
        }))
    }

    /// Only match elements that have the specified property with the specified value.
    /// See the `Needle` documentation for more details on text matching rules.
    pub fn with_property<N>(self, property_name: &str, value: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        let property_name = property_name.to_string();
        self.with_filter(Box::new(move |elem| {
            let property_name = property_name.clone();
            let value = value.clone();
            Box::pin(async move {
                match elem.get_property(&property_name).await {
                    Ok(x) => value.is_match(&x),
                    _ => false,
                }
            })
        }))
    }

    /// Only match elements that have the specified properties with the specified value.
    /// See the `Needle` documentation for more details on text matching rules.
    pub fn with_properties<N>(self, desired_properties: &'static [(&str, N)]) -> Self
    where
        N: Needle + Clone + 'static,
    {
        self.with_filter(Box::new(move |elem| {
            Box::pin(async move {
                for (property_name, value) in desired_properties {
                    match elem.get_property(property_name).await {
                        Ok(x) => {
                            if !value.is_match(&x) {
                                return false;
                            }
                        }
                        _ => return false,
                    }
                }
                true
            })
        }))
    }

    /// Only match elements that have the specified CSS property with the specified value.
    /// See the `Needle` documentation for more details on text matching rules.
    pub fn with_css_property<N>(self, css_property_name: &str, value: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        let css_property_name = css_property_name.to_string();
        self.with_filter(Box::new(move |elem| {
            let css_property_name = css_property_name.clone();
            let value = value.clone();
            Box::pin(async move {
                match elem.get_css_property(&css_property_name).await {
                    Ok(x) => value.is_match(&x),
                    _ => false,
                }
            })
        }))
    }

    /// Only match elements that have the specified CSS properties with the
    /// specified values.
    /// See the `Needle` documentation for more details on text matching rules.
    pub fn with_css_properties<N>(self, desired_css_properties: &'static [(&str, N)]) -> Self
    where
        N: Needle + Clone + 'static,
    {
        self.with_filter(Box::new(move |elem| {
            Box::pin(async move {
                for (css_property_name, value) in desired_css_properties {
                    match elem.get_attribute(css_property_name).await {
                        Ok(x) => {
                            if !value.is_match(&x) {
                                return false;
                            }
                        }
                        _ => return false,
                    }
                }
                true
            })
        }))
    }
}

/// Trait for enabling the ElementQuery interface.
pub trait ElementQueryable {
    fn query<'a>(&'a self, by: By<'a>) -> ElementQuery<'a>;
}

impl ElementQueryable for WebElement<'_> {
    /// Return an ElementQuery instance for more executing powerful element queries.
    fn query<'a>(&'a self, by: By<'a>) -> ElementQuery<'a> {
        let poller: ElementPoller =
            self.session.config().get("ElementPoller").unwrap_or(ElementPoller::NoWait);
        ElementQuery::new(ElementQuerySource::Element(&self), poller, by)
    }
}

impl ElementQueryable for WebDriver {
    /// Return an ElementQuery instance for more executing powerful element queries.
    fn query<'a>(&'a self, by: By<'a>) -> ElementQuery<'a> {
        let poller: ElementPoller =
            self.config().get("ElementPoller").unwrap_or(ElementPoller::NoWait);
        ElementQuery::new(ElementQuerySource::Driver(&self.session), poller, by)
    }
}
