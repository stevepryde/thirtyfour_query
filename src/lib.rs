//! Thirtyfour_query provides an advanced query interface for `thirtyfour`, featuring
//! powerful filtering and polling options.
//!
//! See examples for more details.
//!
//! ## Usage
//!
//! First, set the default polling behaviour:
//! ```rust
//! # use thirtyfour::prelude::*;
//! # use thirtyfour::support::block_on;
//! # use thirtyfour_query::{ElementPoller, ElementQueryable};
//! # use std::time::Duration;
//! #
//! # fn main() -> WebDriverResult<()> {
//! #     block_on(async {
//! #         let caps = DesiredCapabilities::chrome();
//! #         let mut driver = WebDriver::new("http://localhost:4444/wd/hub", &caps).await?;
//! // Disable implicit timeout in order to use new query interface.
//! driver.set_implicit_wait_timeout(Duration::new(0, 0)).await?;
//!
//! let poller = ElementPoller::TimeoutWithInterval(Duration::new(10, 0), Duration::from_millis(500));
//! driver.config_mut().set("ElementPoller", poller)?;
//! #         Ok(())
//! #     })
//! # }
//! ```
//!
//! Other ElementPoller options are also available, such as NoWait and NumTriesWithInterval.
//! These can be overridden on a per-query basis as needed.
//!
//! Now, using the query interface you can do things like:
//!
//! ```rust
//! # use thirtyfour::prelude::*;
//! # use thirtyfour::support::block_on;
//! # use thirtyfour_query::{ElementPoller, ElementQueryable};
//! # use std::time::Duration;
//! #
//! # fn main() -> WebDriverResult<()> {
//! #     block_on(async {
//! #         let caps = DesiredCapabilities::chrome();
//! #         let mut driver = WebDriver::new("http://localhost:4444/wd/hub", &caps).await?;
//! # driver.set_implicit_wait_timeout(Duration::new(0, 0)).await?;
//! #
//! # let poller = ElementPoller::TimeoutWithInterval(Duration::new(10, 0), Duration::from_millis(500));
//! # driver.config_mut().set("ElementPoller", poller)?;
//! # driver.get("http://webappdemo").await?;
//! // This won't wait.
//! let elem_found = driver.query(By::Id("button1")).exists().await?;
//! # assert_eq!(elem_found, true);
//!
//! // This will wait, using the values from ElementPoller above.
//! let elem = driver.query(By::Css("thiswont.match")).or(By::Id("button1")).first().await?;
//! #         assert_eq!(elem.tag_name().await?, "button");
//! #         Ok(())
//! #     })
//! # }
//! ```
//!
//! This will execute both queries once per poll iteration and return the first one that matches.
//!
//! You can also filter on one or both match arms like this:
//!
//! ```rust
//! # use thirtyfour::prelude::*;
//! # use thirtyfour::support::block_on;
//! # use thirtyfour_query::{ElementPoller, ElementQueryable, StringMatch};
//! # use std::time::Duration;
//! #
//! # fn main() -> WebDriverResult<()> {
//! #     block_on(async {
//! #         let caps = DesiredCapabilities::chrome();
//! #         let mut driver = WebDriver::new("http://localhost:4444/wd/hub", &caps).await?;
//! # driver.set_implicit_wait_timeout(Duration::new(0, 0)).await?;
//! #
//! # let poller = ElementPoller::TimeoutWithInterval(Duration::new(10, 0), Duration::from_millis(500));
//! # driver.config_mut().set("ElementPoller", poller)?;
//! # driver.get("http://webappdemo").await?;
//! let elem = driver.query(By::Css("thiswont.match")).with_text("testing")
//!     .or(By::Id("button1")).with_class(StringMatch::new("pure-button").partial()).and_enabled()
//!     .first().await?;
//! #         assert_eq!(elem.tag_name().await?, "button");
//! #         Ok(())
//! #     })
//! # }
//! ```
//!
//! Note the use of `StringMatch` to provide a partial match on the class name.
//! See the documentation for [StringMatch](https://crates.io/crates/stringmatch) for more info.
//!
//! To fetch all matching elements instead of just the first one, simply change `first()` to `all()`
//! and you'll get a Vec instead. Also see `all_required()` if you want it to return an error
//! when there are no matching elements.
//!
//! All timeout, interval and ElementPoller details can be overridden on a per-call basis if
//! desired. See the `ElementQuery` documentation for more details.
//!
mod query;
pub use query::*;

/// This is a re-export of stringmatch::StringMatch.
pub use stringmatch::StringMatch;
