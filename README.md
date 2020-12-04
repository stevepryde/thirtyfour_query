[![Crates.io](https://img.shields.io/crates/v/thirtyfour_query.svg?style=for-the-badge)](https://crates.io/crates/thirtyfour_query)
[![docs.rs](https://img.shields.io/badge/docs.rs-thirtyfour_query-blue?style=for-the-badge)](https://docs.rs/thirtyfour_query)

Advanced element query interface for the thirtyfour crate.

## Usage

First, set the default polling behaviour:
```rust 
// Disable implicit timeout in order to use new query interface.
driver.set_implicit_wait_timeout(Duration::new(0, 0)).await?;

let poller = ElementPoller::TimeoutWithInterval(Duration::new(20, 0), Duration::from_millis(500));
driver.config_mut().set("ElementPoller", poller)?;
```

Other ElementPoller options are also available, such as NoWait and NumTriesWithInterval.
These can be overridden on a per-query basis as needed.

Now, using the query interface you can do things like:

```rust
let elem_text = 
    driver.query(By::Css("thiswont.match")).or(By::Id("searchInput")).first().await?;
```
    
This will execute both queries once per poll iteration and return the first one that matches.
You can also filter on one or both match arms like this:

```rust
driver.query(By::Css("thiswont.match")).with_text("testing")
    .or(By::Id("searchInput")).with_class("search").and_not_enabled()
    .first().await?;
```

To fetch all matching elements instead of just the first one, simply change first() to all() 
and you'll get a Vec instead.

## LICENSE

This work is dual-licensed under MIT or Apache 2.0.
You can choose either license if you use this work.

`SPDX-License-Identifier: MIT OR Apache-2.0`
