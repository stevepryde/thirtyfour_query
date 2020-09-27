use tokio::time::{delay_for, Duration, Instant};

use serde::{Deserialize, Serialize};
use std::mem;
use thirtyfour::error::{WebDriverError, WebDriverErrorInfo};
use thirtyfour::prelude::{WebDriver, WebDriverResult};
use thirtyfour::{By, WebDriverCommands, WebDriverSession, WebElement};

fn get_selector_summary(selectors: &Vec<ElementSelector>) -> String {
    let criteria: Vec<String> = selectors.iter().map(|s| s.by.to_string()).collect();
    format!("[{}]", criteria.join(","))
}

fn no_such_element(selectors: &Vec<ElementSelector>) -> WebDriverError {
    WebDriverError::NoSuchElement(WebDriverErrorInfo::new(&get_selector_summary(selectors)))
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

pub struct ElementSelector<'a> {
    pub by: By<'a>,
    pub filters: Vec<Box<dyn Fn(Vec<WebElement<'a>>) -> Vec<WebElement<'a>>>>,
}

impl<'a> ElementSelector<'a> {
    pub fn new(by: By<'a>) -> Self {
        Self {
            by: by.clone(),
            filters: Vec::new(),
        }
    }

    pub fn add_filter(&mut self, f: Box<dyn Fn(Vec<WebElement<'a>>) -> Vec<WebElement<'a>>>) {
        self.filters.push(f);
    }
}

pub struct ElementQuery<'a> {
    session: &'a WebDriverSession,
    poller: ElementPoller,
    selectors: Vec<ElementSelector<'a>>,
}

impl<'a> ElementQuery<'a> {
    pub fn new(session: &'a WebDriverSession, poller: ElementPoller, by: By<'a>) -> Self {
        let selector = ElementSelector::new(by.clone());
        Self {
            session,
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

    pub fn or_by(mut self, by: By<'a>) -> Self {
        let selector = ElementSelector::new(by);
        self.selectors.push(selector);
        self
    }

    pub fn and_filter(
        mut self,
        f: Box<dyn Fn(Vec<WebElement<'a>>) -> Vec<WebElement<'a>>>,
    ) -> Self {
        if let Some(selector) = self.selectors.last_mut() {
            selector.add_filter(f);
        }
        self
    }

    pub async fn first(mut self) -> WebDriverResult<WebElement<'a>> {
        let mut elements = {
            let selectors = mem::replace(&mut self.selectors, Vec::new());
            run_poller(self.poller, self.session, selectors).await
        }?;

        if elements.is_empty() {
            let err = WebDriverError::NotFoundError("Element not found".to_string());
            Err(err)
        } else {
            Ok(elements.remove(0))
        }
    }

    pub async fn all(mut self) -> WebDriverResult<Vec<WebElement<'a>>> {
        let elements = {
            let selectors = mem::replace(&mut self.selectors, Vec::new());
            run_poller(self.poller, self.session, selectors).await
        }?;
        Ok(elements)
    }
}

pub trait ElementQueryable<'a> {
    fn query(&'a self, by: By<'a>) -> ElementQuery<'a>;
}

impl<'a> ElementQueryable<'a> for WebElement<'a> {
    fn query(&'a self, by: By<'a>) -> ElementQuery<'a> {
        let poller: ElementPoller =
            self.session.config().get("ElementPoller").unwrap_or(ElementPoller::NoWait);
        ElementQuery::new(&self.session, poller, by)
    }
}

impl<'a> ElementQueryable<'a> for WebDriver {
    fn query(&'a self, by: By<'a>) -> ElementQuery<'a> {
        let poller: ElementPoller =
            self.config().get("ElementPoller").unwrap_or(ElementPoller::NoWait);
        ElementQuery::new(&self.session, poller, by)
    }
}

pub async fn run_poller<'a>(
    poller: ElementPoller,
    session: &'a WebDriverSession,
    selectors: Vec<ElementSelector<'a>>,
) -> WebDriverResult<Vec<WebElement<'a>>> {
    match &poller {
        ElementPoller::NoWait => run_poller_with_options(session, None, None, 0, selectors).await,
        ElementPoller::Time(timeout, interval) => {
            run_poller_with_options(
                session,
                Some(timeout.clone()),
                Some(interval.clone()),
                0,
                selectors,
            )
            .await
        }
        ElementPoller::NumTries(max_tries, interval) => {
            run_poller_with_options(session, None, Some(interval.clone()), *max_tries, selectors)
                .await
        }
    }
}

async fn run_poller_with_options<'a>(
    session: &'a WebDriverSession,
    timeout: Option<Duration>,
    interval: Option<Duration>,
    max_tries: u32,
    selectors: Vec<ElementSelector<'a>>,
) -> WebDriverResult<Vec<WebElement<'a>>> {
    let no_such_element_error = no_such_element(&selectors);
    if selectors.is_empty() {
        return Err(no_such_element_error);
    }
    let mut tries = 0;

    let start = Instant::now();
    loop {
        tries += 1;

        for selector in selectors.iter() {
            let mut elements = match session.find_elements(selector.by.clone()).await {
                Ok(x) => x,
                Err(WebDriverError::NoSuchElement(_)) => Vec::new(),
                Err(e) => return Err(e),
            };

            if !elements.is_empty() {
                for f in &selector.filters {
                    elements = f(elements);
                    if elements.is_empty() {
                        break;
                    }
                }
            }

            if !elements.is_empty() {
                return Ok(elements);
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
