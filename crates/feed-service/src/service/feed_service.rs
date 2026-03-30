use std::sync::Arc;

use uuid::Uuid;

use shared::errors::AppResult;

use crate::{
    domain::models::{FeedItem, FeedResponse},
    service::fanout_service::FanoutService,
};

pub struct FeedService {
    fanout: Arc<FanoutService>,
    feed_page_size: i32,
}

impl FeedService {
    pub fn new(fanout: Arc<FanoutService>, feed_page_size: i32) -> Self {
        Self {
            fanout,
            feed_page_size,
        }
    }

    /// Retrieve paginated feed for a user.
    ///
    /// `cursor` is the score (timestamp_ms) of the last item from the previous page.
    /// Client passes it back as `before_score` on subsequent requests.
    pub async fn get_feed(
        &self,
        user_id: Uuid,
        cursor: Option<f64>,
        limit: Option<i32>,
    ) -> AppResult<FeedResponse> {
        let page_size = limit
            .unwrap_or(self.feed_page_size)
            .min(100)
            .max(1);

        // Fetch one extra to detect if there are more pages
        let items = self
            .fanout
            .build_feed(user_id, cursor, page_size + 1)
            .await?;

        let has_more = items.len() > page_size as usize;
        let mut items = items;
        if has_more {
            items.truncate(page_size as usize);
        }

        let next_cursor = if has_more {
            items.last().map(|item| item.score)
        } else {
            None
        };

        Ok(FeedResponse {
            items,
            next_cursor,
            has_more,
        })
    }
}
