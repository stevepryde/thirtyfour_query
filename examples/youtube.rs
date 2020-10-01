//! Requires chromedriver running on port 4444:
//!
//!     chromedriver --port=4444
//!
//! Run as follows:
//!
//!     cargo run --example youtube

use regex::Regex;
use stringmatch::StringMatch;
use thirtyfour::prelude::*;
use thirtyfour_query::query::{ElementPoller, ElementQueryable};
use tokio;
use tokio::time::{delay_for, Duration};
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

    // Navigate to https://youtube.com
    driver.get("https://youtube.com").await?;
    let elem_search = driver.query(By::Css("#search-input #search")).first().await?;

    // Type in the search terms.
    elem_search.send_keys("rick astley never gonna give you up").await?;
    elem_search.send_keys(Keys::Enter).await?;

    // Find the first video in the list matching the desired title and click it.
    let elem_title = driver
        .query(By::Css(
            "#page-manager .text-wrapper #title-wrapper #video-title .ytd-video-renderer",
        ))
        .with_text(StringMatch::from("Never Gonna Give You Up").partial())
        .first()
        .await?;

    // Click the parent element.
    elem_title.query(By::XPath("./..")).first().await?.click().await?;

    // Make it full-screen
    let elem_fullscreen_button =
        driver.query(By::Css("button.ytp-fullscreen-button")).first().await?;
    elem_fullscreen_button.scroll_into_view().await?;
    delay_for(Duration::new(1, 0)).await;
    elem_fullscreen_button.click().await?;

    // Wait for it to finish. We can find the exact number of seconds in the DOM.
    let progress_bar = driver
        .query(By::ClassName("ytp-progress-bar"))
        .with_attribute("aria-valuemax", Regex::new(r"\d+").unwrap())
        .first()
        .await?;
    let seconds: u64 = progress_bar.get_attribute("aria-valuemax").await?.parse().unwrap_or(30);
    delay_for(Duration::new(seconds, 0)).await;

    Ok(())
}
