use std::sync::Arc;

use druid::{piet::PietImage, ExtEventSink, Target};
use lapce_core::directory::Directory;
use sha2::{Digest, Sha256};
use url::Url;

use crate::command::{LapceUICommand, LAPCE_UI_COMMAND};

#[derive(Clone)]
pub enum Image {
    Image(Arc<PietImage>),
}
impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Image::Image(_) => f.debug_tuple("Image").finish(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ImageStatus {
    /// The image is being requested
    Loading,
    // TODO: Give this some error string to display? Maybe use thiserror or whatever?
    /// There was some error in loading the image
    Error,
    /// The image has been loaded
    Loaded(Image),
}

#[derive(Clone)]
struct ImageCacheEntry {
    /// The number of active entries for this image, used for reference counting
    pub count: usize,
    pub image: ImageStatus,
}

#[derive(Clone, Default)]
pub struct ImageCache {
    images: im::HashMap<Url, ImageCacheEntry>,
}
impl ImageCache {
    /// Loads the given url, and submits the `ImageLoaded` event to the event sink when it's done.  
    /// You can use [`ImageCache::get`] to get the image's status, and content once it is finished.
    pub fn load_url_cmd(&mut self, url: Url, event_sink: ExtEventSink) {
        self.load_url_cb(url.clone(), move |image| {
            let _ = event_sink.submit_command(
                LAPCE_UI_COMMAND,
                LapceUICommand::ImageLoaded { url, image },
                Target::Auto,
            );
        });
    }

    fn load_url_cb(
        &mut self,
        url: Url,
        cb: impl FnOnce(anyhow::Result<Image>) + Send + 'static,
    ) {
        let has_img = self.images.contains_key(&url);
        let img =
            self.images
                .entry(url.clone())
                .or_insert_with(|| ImageCacheEntry {
                    count: 0,
                    image: ImageStatus::Loading,
                });
        img.count += 1;

        // If we've already started loading the image, then assume that we don't need to load it
        // again. If this is incorrect, then that's a sign of some bug in the code which uses
        // the cache. Either forgetting to call `load_finished` or accidentally unregistering
        // an image too many times.
        if has_img {
            return;
        }

        std::thread::spawn(move || cb(get_image(url)));
    }

    pub fn load_finished(
        &mut self,
        url: &Url,
        image: &Result<Image, anyhow::Error>,
    ) {
        let image = match image {
            Ok(image) => ImageStatus::Loaded(image.clone()),
            Err(_) => ImageStatus::Error,
        };

        if let Some(img) = self.images.get_mut(url) {
            img.image = image;
        } else {
            log::warn!("Image loading for '{url}' finished, but it was not present in the cache.");
            self.images
                .insert(url.clone(), ImageCacheEntry { count: 1, image });
        }
    }

    pub fn done_with_image(&mut self, url: &Url) {
        if let Some(img) = self.images.get_mut(url) {
            img.count = img.count.saturating_sub(1);
        } else {
            log::warn!("Image '{url}' was not present in the cache.");
        }
    }

    pub fn get(&self, url: &Url) -> Option<&ImageStatus> {
        self.images.get(url).map(|x| &x.image)
    }
}

fn get_image(url: Url) -> Result<Image, anyhow::Error> {
    // Load the image's raw bytes
    let content = if url.scheme() == "file" {
        // If it's a file, then we can just load it directly rather than
        // storing it in a cache directory.
        let path = url.to_file_path().unwrap();
        std::fs::read(path)?
    } else {
        // Hash the url to get a safe and basically-unique filename
        let cache_file_path = Directory::cache_directory().map(|cache_dir| {
            let mut hasher = Sha256::new();
            hasher.update(url.as_str().as_bytes());
            let filename = format!("{:x}", hasher.finalize());
            cache_dir.join(filename)
        });

        let cache_content =
            cache_file_path.as_ref().and_then(|p| std::fs::read(p).ok());

        match cache_content {
            Some(content) => content,
            None => {
                let resp = reqwest::blocking::get(url.clone())?;
                if !resp.status().is_success() {
                    return Err(anyhow::anyhow!("can't download icon"));
                }
                let buf = resp.bytes()?.to_vec();

                if let Some(path) = cache_file_path.as_ref() {
                    let _ = std::fs::write(path, &buf);
                }

                buf
            }
        }
    };

    let image = PietImage::from_bytes(&content)
        .map_err(|_| anyhow::anyhow!("can't resolve image from '{url}'"))?;
    Ok(Image::Image(Arc::new(image)))
}
