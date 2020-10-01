use tokio::time::{delay_for, Duration, Instant};

use futures::Future;
use serde::{Deserialize, Serialize};
use std::mem;
use std::pin::Pin;
use stringmatch::Needle;
use thirtyfour::error::{WebDriverError, WebDriverErrorInfo};
use thirtyfour::prelude::{WebDriver, WebDriverResult};
use thirtyfour::{By, WebDriverCommands, WebDriverSession, WebElement};

fn get_selector_summary(selectors: &Vec<ElementSelector>) -> String {
    let criteria: Vec<String> = selectors.iter().map(|s| s.by.to_string()).collect();
    format!("[{}]", criteria.join(","))
}

fn no_such_element(selectors: &Vec<ElementSelector>) -> WebDriverError {
    WebDriverError::NoSuchElement(WebDriverErrorInfo::new(&format!(
        "Element(s) not found using selectors: {}",
        &get_selector_summary(selectors)
    )))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ElementPoller {
    NoWait,
    Time(Duration, Duration),
    NumTries(u32, Duration),
}

impl ElementPoller {
    pub fn with_timeout(timeout: Duration, interval: Duration) -> Self {
        Self::Time(timeout, interval)
    }

    pub fn with_max_tries(max_tries: u32, interval: Duration) -> Self {
        Self::NumTries(max_tries, interval)
    }
}
type ElementFilter =
    Box<dyn for<'a> Fn(&'a WebElement<'a>) -> Pin<Box<dyn Future<Output = bool> + 'a>>>;

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

    pub fn set_single(&mut self) {
        self.single = true;
    }

    pub fn add_filter(&mut self, f: ElementFilter) {
        self.filters.push(f);
    }

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

pub enum ElementQuerySource<'a> {
    Driver(&'a WebDriverSession),
    Element(&'a WebElement<'a>),
}

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

    pub fn wait(mut self, timeout: Duration, interval: Duration) -> Self {
        self.poller = ElementPoller::with_timeout(timeout, interval);
        self
    }

    pub fn nowait(mut self) -> Self {
        self.poller = ElementPoller::NoWait;
        self
    }

    pub fn add_selector(mut self, selector: ElementSelector<'a>) -> Self {
        self.selectors.push(selector);
        self
    }

    pub fn or(self, by: By<'a>) -> Self {
        self.add_selector(ElementSelector::new(by))
    }

    pub async fn first(mut self) -> WebDriverResult<WebElement<'a>> {
        let mut elements = self.run_poller().await?;

        if elements.is_empty() {
            Err(no_such_element(&self.selectors))
        } else {
            Ok(elements.remove(0))
        }
    }

    pub async fn all(mut self) -> WebDriverResult<Vec<WebElement<'a>>> {
        self.run_poller().await
    }

    async fn run_poller(&mut self) -> WebDriverResult<Vec<WebElement<'a>>> {
        match self.poller {
            ElementPoller::NoWait => self.run_poller_with_options(None, None, 0).await,
            ElementPoller::Time(timeout, interval) => {
                self.run_poller_with_options(Some(timeout.clone()), Some(interval.clone()), 0).await
            }
            ElementPoller::NumTries(max_tries, interval) => {
                self.run_poller_with_options(None, Some(interval.clone()), max_tries).await
            }
        }
    }

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

    async fn run_poller_with_options(
        &mut self,
        timeout: Option<Duration>,
        interval: Option<Duration>,
        max_tries: u32,
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
                    if start.elapsed() >= t {
                        return Err(no_such_element_error);
                    }
                }
            }

            if timeout.is_none() && tries >= max_tries {
                return Err(no_such_element_error);
            }

            if let Some(i) = interval {
                delay_for(i).await;
            }
        }
    }

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

    pub fn with_text<N>(self, text: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        let text = text.clone();
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

    pub fn with_id<N>(self, id: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        let id = id.clone();
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

    pub fn with_class<N>(self, class_name: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        let class_name = class_name.clone();
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

    pub fn with_tag<N>(self, tag_name: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        let tag_name = tag_name.clone();
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

    pub fn with_value<N>(self, value: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        let value = value.clone();
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

    pub fn with_attribute<N>(self, attribute_name: &str, value: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        let attribute_name = attribute_name.to_string();
        let value = value.clone();
        self.with_filter(Box::new(move |elem| {
            let attribute_name = attribute_name.to_string();
            let value = value.clone();
            Box::pin(async move {
                match elem.get_attribute(&attribute_name).await {
                    Ok(x) => value.is_match(&x),
                    _ => false,
                }
            })
        }))
    }

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

    pub fn with_property<N>(self, property_name: &str, value: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        let property_name = property_name.to_string();
        let value = value.clone();
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

    pub fn with_css_property<N>(self, css_property_name: &str, value: N) -> Self
    where
        N: Needle + Clone + 'static,
    {
        let css_property_name = css_property_name.to_string();
        let value = value.clone();
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

pub trait ElementQueryable<'a> {
    fn query(&'a self, by: By<'a>) -> ElementQuery<'a>;
}

impl<'a> ElementQueryable<'a> for WebElement<'a> {
    fn query(&'a self, by: By<'a>) -> ElementQuery<'a> {
        let poller: ElementPoller =
            self.session.config().get("ElementPoller").unwrap_or(ElementPoller::NoWait);
        ElementQuery::new(ElementQuerySource::Element(&self), poller, by)
    }
}

impl<'a> ElementQueryable<'a> for WebDriver {
    fn query(&'a self, by: By<'a>) -> ElementQuery<'a> {
        let poller: ElementPoller =
            self.config().get("ElementPoller").unwrap_or(ElementPoller::NoWait);
        ElementQuery::new(ElementQuerySource::Driver(&self.session), poller, by)
    }
}
