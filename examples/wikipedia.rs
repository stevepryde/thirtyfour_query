//! Requires chromedriver running on port 4444:
//!
//!     chromedriver --port=4444
//!
//! Run as follows:
//!
//!     cargo run --example wikipedia

use thirtyfour::prelude::*;
use thirtyfour_query::query::{ElementPoller, ElementQueryable};
use tokio;
use tokio::time::Duration;

#[tokio::main]
async fn main() -> WebDriverResult<()> {
    let caps = DesiredCapabilities::chrome();
    let mut driver = WebDriver::new("http://localhost:4444", &caps).await?;

    // Disable implicit timeout in order to use new query interface.
    driver.set_implicit_wait_timeout(Duration::new(0, 0)).await?;

    // Set default ElementPoller strategy. This will be inherited by all future queries unless
    // specifically overridden.
    // The following will wait up to 20 seconds, polling in 0.5 second intervals.
    let poller = ElementPoller::Time(Duration::new(20, 0), Duration::from_millis(500));
    driver.config_mut().set("ElementPoller", poller)?;

    // Navigate to https://wikipedia.org.
    driver.get("https://wikipedia.org").await?;
    let elem_form = driver.query(By::Id("search-form")).first().await?;

    // Find element from element using multiple selectors.
    // Each selector will be executed once per poll iteration.
    //The first element to match will be returned.
    let elem_text =
        elem_form.query(By::Css("thiswont.match")).or(By::Id("searchInput")).first().await?;

    // Type in the search terms.
    elem_text.send_keys("selenium").await?;

    // Click the search button.
    let elem_button = elem_form.query(By::Css("button[type='submit']")).first().await?;
    elem_button.click().await?;

    // Look for header to implicitly wait for the page to load.
    driver.query(By::ClassName("firstHeading")).first().await?;
    assert_eq!(driver.title().await?, "Selenium - Wikipedia");

    Ok(())
}
