#![allow(unused_imports)] // Suppress unused-imports warnings

use eframe::{App, Frame, NativeOptions};
use eframe::egui::{CentralPanel, ScrollArea, vec2, Slider, TextEdit, TopBottomPanel, Visuals, Color32, ProgressBar, Ui, RichText, Label, Style, ComboBox};
use reqwest::blocking::Client;
use scraper::{Html, Selector};
use url::Url;
use serde::{Serialize, Deserialize};
use serde_json;
use csv::Writer;
use rayon::prelude::*;
use crossbeam_channel::{bounded, Sender, Receiver};
use rfd::FileDialog;
use chrono::Local;
use std::sync::{Arc, Mutex};
use std::path::Path;
use std::fs::{File, self};
use std::io::{self, BufRead, BufReader, Write};
use std::time::Duration;
use regex::Regex;
use headless_chrome::{Browser, browser::LaunchOptions};
use std::collections::{HashSet, VecDeque};

#[derive(Serialize, Deserialize)]
struct ScrapedData {
    url: String,
    content: String,
    attributes: String,
}

#[derive(Serialize, Deserialize)]
struct ScraperConfig {
    url_input: String,
    selector_input: String,
    attribute_input: String,
    regex_input: String,
    timeout_secs: f32,
    crawl_depth: f32,
    next_page_selector: String,
    custom_headers: String,
    proxy: String,
    max_concurrent: f32,
    content_type: String,
    retry_attempts: f32,
    scrape_delay: f32,
    use_headless: bool,
}

struct ScraperApp {
    url_input: String,
    selector_input: String,
    attribute_input: String,
    regex_input: String,
    timeout_secs: f32,
    crawl_depth: f32,
    next_page_selector: String,
    custom_headers: String,
    proxy: String,
    max_concurrent: f32,
    content_type: String,
    retry_attempts: f32,
    scrape_delay: f32,
    use_headless: bool,
    results: Arc<Mutex<Vec<ScrapedData>>>,
    status: String,
    log: Arc<Mutex<Vec<String>>>,
    progress: Arc<Mutex<f32>>,
    total_urls: Arc<Mutex<usize>>,
    tx: Sender<String>,
    rx: Receiver<String>,
    dark_mode: bool,
    last_max_concurrent: f32,
}

impl ScraperApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Initialize Rayon thread pool once
        rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build_global()
            .unwrap_or_else(|e| eprintln!("Failed to initialize thread pool: {}", e));

        let (tx, rx) = bounded(100);
        Self {
            url_input: String::new(),
            selector_input: "p, h1, h2, h3".to_string(),
            attribute_input: "".to_string(),
            regex_input: "".to_string(),
            timeout_secs: 10.0,
            crawl_depth: 1.0,
            next_page_selector: "".to_string(),
            custom_headers: "".to_string(),
            proxy: "".to_string(),
            max_concurrent: 4.0,
            content_type: "text".to_string(),
            retry_attempts: 2.0,
            scrape_delay: 1.0,
            use_headless: true,
            results: Arc::new(Mutex::new(Vec::new())),
            status: "Ready to scrape".to_string(),
            log: Arc::new(Mutex::new(Vec::new())),
            progress: Arc::new(Mutex::new(0.0)),
            total_urls: Arc::new(Mutex::new(0)),
            tx,
            rx,
            dark_mode: true,
            last_max_concurrent: 4.0,
        }
    }

    fn normalize_url(url: &str) -> String {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            format!("https://{}", url)
        } else {
            url.to_string()
        }
    }

    fn scrape_url(
        url: &str,
        client: &Client,
        selector: &str,
        attribute: &Option<String>,
        regex: &Option<Regex>,
        use_headless: bool,
        content_type: &str,
    ) -> Result<ScrapedData, String> {
        let normalized_url = Self::normalize_url(url);
        let parsed_url = Url::parse(&normalized_url).map_err(|e| format!("Invalid URL {}: {}", normalized_url, e))?;

        let html = if use_headless {
            let launch_options = LaunchOptions::default_builder()
                .headless(true)
                .build()
                .map_err(|e| format!("Failed to build launch options: {}", e.to_string()))?;
            let browser = Browser::new(launch_options)
                .map_err(|e| format!("Failed to start headless browser: {}", e.to_string()))?;
            let tab = browser.new_tab()
                .map_err(|e| format!("Failed to create tab: {}", e.to_string()))?;
            tab.navigate_to(&normalized_url)
                .map_err(|e| format!("Failed to navigate to {}: {}", normalized_url, e.to_string()))?;
            tab.wait_until_navigated()
                .map_err(|e| format!("Navigation failed for {}: {}", normalized_url, e.to_string()))?;
            tab.get_content()
                .map_err(|e| format!("Failed to get content for {}: {}", normalized_url, e.to_string()))
        } else {
            client
                .get(parsed_url.as_str())
                .send()
                .map_err(|e| format!("Failed to fetch {}: {}", normalized_url, e))?
                .text()
                .map_err(|e| format!("Failed to read response from {}: {}", normalized_url, e))
        }?;

        // Parse HTML
        let document = Html::parse_document(&html);
        let selector_obj = Selector::parse(selector).map_err(|e| format!("Invalid selector {}: {:?}", selector, e))?;
        let elements: Vec<_> = document.select(&selector_obj).collect();

        if elements.is_empty() {
            return Err(format!("No elements found for selector '{}' on {}", selector, normalized_url));
        }

        let mut content;
        let mut attributes = String::new();

        match content_type.to_lowercase().as_str() {
            "text" => {
                content = elements
                    .iter()
                    .map(|element| element.text().collect::<Vec<_>>().join(" "))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            "links" => {
                content = elements
                    .iter()
                    .filter_map(|element| element.value().attr("href"))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            "images" => {
                content = elements
                    .iter()
                    .filter_map(|element| element.value().attr("src"))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            _ => return Err(format!("Invalid content type: {}", content_type)),
        }

        // Apply regex filtering
        if let Some(regex) = regex {
            let matches = regex
                .find_iter(&content)
                .map(|m| m.as_str().to_string())
                .collect::<Vec<_>>();
            content = if matches.is_empty() {
                return Err(format!("No regex matches found for {} on {}", regex, normalized_url));
            } else {
                matches.join("\n")
            };
        }

        // Extract additional attributes for text content type
        if content_type == "text" {
            if let Some(attr) = attribute {
                attributes = elements
                    .iter()
                    .filter_map(|element| element.value().attr(attr))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }

        if content.is_empty() {
            return Err(format!("No content extracted for {} on {}", content_type, normalized_url));
        }

        Ok(ScrapedData {
            url: normalized_url,
            content,
            attributes,
        })
    }

    fn save_to_csv(&self, filename: &str) -> Result<(), String> {
        let results = self.results.lock().map_err(|e| format!("Mutex error: {}", e))?;
        if results.is_empty() {
            return Err("No data to save".to_string());
        }

        let mut writer = Writer::from_path(Path::new(filename))
            .map_err(|e| format!("Failed to create CSV file {}: {}", filename, e))?;

        for data in results.iter() {
            writer.serialize(data)
                .map_err(|e| format!("Failed to write to CSV: {}", e))?;
        }

        writer.flush().map_err(|e| format!("Failed to flush CSV: {}", e))?;
        Ok(())
    }

    fn save_to_json(&self, filename: &str) -> Result<(), String> {
        let results = self.results.lock().map_err(|e| format!("Mutex error: {}", e))?;
        if results.is_empty() {
            return Err("No data to save".to_string());
        }

        let json = serde_json::to_string_pretty(&*results)
            .map_err(|e| format!("Failed to serialize JSON: {}", e))?;
        fs::write(filename, json).map_err(|e| format!("Failed to write JSON file {}: {}", filename, e))?;
        Ok(())
    }

    fn load_urls_from_file(&self, filename: &str) -> Result<Vec<String>, String> {
        let file = File::open(filename).map_err(|e| format!("Failed to open file {}: {}", filename, e))?;
        let reader = BufReader::new(file);
        let urls: Vec<String> = reader
            .lines()
            .filter_map(|line| line.ok())
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();
        if urls.is_empty() {
            Err("No valid URLs found in file".to_string())
        } else {
            Ok(urls)
        }
    }

    fn save_config(&self, filename: &str) -> Result<(), String> {
        let config = ScraperConfig {
            url_input: self.url_input.clone(),
            selector_input: self.selector_input.clone(),
            attribute_input: self.attribute_input.clone(),
            regex_input: self.regex_input.clone(),
            timeout_secs: self.timeout_secs,
            crawl_depth: self.crawl_depth,
            next_page_selector: self.next_page_selector.clone(),
            custom_headers: self.custom_headers.clone(),
            proxy: self.proxy.clone(),
            max_concurrent: self.max_concurrent,
            content_type: self.content_type.clone(),
            retry_attempts: self.retry_attempts,
            scrape_delay: self.scrape_delay,
            use_headless: self.use_headless,
        };
        let json = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        fs::write(filename, json).map_err(|e| format!("Failed to write config file {}: {}", filename, e))?;
        Ok(())
    }

    fn load_config(&mut self, filename: &str) -> Result<(), String> {
        let json = fs::read_to_string(filename).map_err(|e| format!("Failed to read config file {}: {}", filename, e))?;
        let config: ScraperConfig = serde_json::from_str(&json)
            .map_err(|e| format!("Failed to deserialize config: {}", e))?;
        self.url_input = config.url_input;
        self.selector_input = config.selector_input;
        self.attribute_input = config.attribute_input;
        self.regex_input = config.regex_input;
        self.timeout_secs = config.timeout_secs;
        self.crawl_depth = config.crawl_depth;
        self.next_page_selector = config.next_page_selector;
        self.custom_headers = config.custom_headers;
        self.proxy = config.proxy;
        self.max_concurrent = config.max_concurrent;
        self.content_type = config.content_type;
        self.retry_attempts = config.retry_attempts;
        self.scrape_delay = config.scrape_delay;
        self.use_headless = config.use_headless;
        Ok(())
    }

    fn parse_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        for line in self.custom_headers.lines() {
            let parts: Vec<&str> = line.splitn(2, ':').map(|s| s.trim()).collect();
            if parts.len() == 2 {
                if let Ok(name) = reqwest::header::HeaderName::from_bytes(parts[0].as_bytes()) {
                    if let Ok(value) = reqwest::header::HeaderValue::from_str(parts[1]) {
                        headers.insert(name, value);
                    }
                }
            }
        }
        headers
    }
}

impl App for ScraperApp {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut Frame) {
        // Apply theme
        ctx.set_visuals(if self.dark_mode {
            Visuals::dark()
        } else {
            Visuals::light()
        });

        // Check for status updates
        while let Ok(status) = self.rx.try_recv() {
            self.status = status.clone();
            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            self.log.lock().unwrap().push(format!("[{}] {}", timestamp, status));
            println!("Status: {}", self.status);
            ctx.request_repaint();
        }

        // Set global UI style
        let mut style = ctx.style().as_ref().clone();
        style.spacing.item_spacing = vec2(10.0, 10.0);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(50, 50, 50);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(80, 80, 80);
        ctx.set_style(style);

        // Top panel for theme toggle and config
        TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add(Label::new(RichText::new("Website Scraper").heading().color(Color32::from_rgb(100, 200, 255))));
                ui.add_space(20.0);
                if ui.button("Toggle Theme").clicked() {
                    self.dark_mode = !self.dark_mode;
                }
                if ui.button("Save Config").clicked() {
                    if let Some(path) = FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .set_file_name("scraper_config.json")
                        .save_file()
                    {
                        let filename = path.to_str().unwrap_or("scraper_config.json");
                        match self.save_config(filename) {
                            Ok(()) => {
                                self.status = format!("Config saved to {}", filename);
                                self.log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), self.status));
                            }
                            Err(e) => {
                                self.status = format!("Failed to save config: {}", e);
                                self.log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), self.status));
                            }
                        }
                        ctx.request_repaint();
                    }
                }
                if ui.button("Load Config").clicked() {
                    if let Some(path) = FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .pick_file()
                    {
                        let filename = path.to_str().unwrap_or("");
                        match self.load_config(filename) {
                            Ok(()) => {
                                self.status = format!("Config loaded from {}", filename);
                                self.log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), self.status));
                            }
                            Err(e) => {
                                self.status = format!("Failed to load config: {}", e);
                                self.log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), self.status));
                            }
                        }
                        ctx.request_repaint();
                    }
                }
            });
        });

        CentralPanel::default().show(ctx, |ui| {
            // Input section
            ui.collapsing("Input Settings", |ui| {
                ui.horizontal(|ui| {
                    ui.label("URLs (comma-separated): ");
                    ui.text_edit_singleline(&mut self.url_input)
                        .on_hover_text("Enter URLs like https://example.com, https://github.com");
                });
                if ui.button("Load URLs from File").clicked() {
                    if let Some(path) = FileDialog::new()
                        .add_filter("Text", &["txt"])
                        .pick_file()
                    {
                        match self.load_urls_from_file(path.to_str().unwrap_or("")) {
                            Ok(urls) => {
                                self.url_input = urls.join(", ");
                                self.status = format!("Loaded {} URLs from file", urls.len());
                                self.log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), self.status));
                            }
                            Err(e) => {
                                self.status = e;
                                self.log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), self.status));
                            }
                        }
                        ctx.request_repaint();
                    }
                }
            });

            // Scraping settings
            ui.collapsing("Scraping Settings", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Content Type: ");
                    ComboBox::from_label("")
                        .selected_text(&self.content_type)
                        .show_ui(ui, |ui| {
                            if ui.selectable_value(&mut self.content_type, "text".to_string(), "Text").clicked() {
                                self.selector_input = "p, h1, h2, h3".to_string();
                                self.attribute_input = "".to_string();
                            }
                            if ui.selectable_value(&mut self.content_type, "links".to_string(), "Links").clicked() {
                                self.selector_input = "a".to_string();
                                self.attribute_input = "href".to_string();
                            }
                            if ui.selectable_value(&mut self.content_type, "images".to_string(), "Images").clicked() {
                                self.selector_input = "img".to_string();
                                self.attribute_input = "src".to_string();
                            }
                        })
                        .response
                        .on_hover_text("Select content type: text, links, or images");
                });
                ui.horizontal(|ui| {
                    ui.label("CSS Selector: ");
                    ui.text_edit_singleline(&mut self.selector_input)
                        .on_hover_text("e.g., p, h1, div.my-class for text; a for links; img for images");
                });
                ui.horizontal(|ui| {
                    ui.label("HTML Attribute: ");
                    ui.text_edit_singleline(&mut self.attribute_input)
                        .on_hover_text("e.g., href for links, src for images, leave blank for text");
                });
                ui.horizontal(|ui| {
                    ui.label("Regex Filter: ");
                    ui.text_edit_singleline(&mut self.regex_input)
                        .on_hover_text("e.g., [a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}");
                });
                ui.horizontal(|ui| {
                    ui.label("Next Page Selector: ");
                    ui.text_edit_singleline(&mut self.next_page_selector)
                        .on_hover_text("e.g., a.next, leave blank for none");
                });
                ui.horizontal(|ui| {
                    ui.label("Crawl Depth: ");
                    ui.add(Slider::new(&mut self.crawl_depth, 0.0..=5.0).step_by(1.0))
                        .on_hover_text("0 = no crawling, 1-5 = follow links up to depth");
                });
                ui.horizontal(|ui| {
                    ui.label("Custom Headers: ");
                    ui.add(TextEdit::multiline(&mut self.custom_headers).desired_rows(3))
                        .on_hover_text("key:value, one per line, e.g., Cookie: key=value");
                });
                ui.horizontal(|ui| {
                    ui.label("Proxy (e.g., socks5://user:pass@host:port): ");
                    ui.text_edit_singleline(&mut self.proxy)
                        .on_hover_text("Leave blank for no proxy");
                });
                ui.horizontal(|ui| {
                    ui.label("Max Concurrent Requests: ");
                    ui.add(Slider::new(&mut self.max_concurrent, 1.0..=10.0).step_by(1.0))
                        .on_hover_text("Number of simultaneous requests");
                });
                ui.horizontal(|ui| {
                    ui.label("Retry Attempts: ");
                    ui.add(Slider::new(&mut self.retry_attempts, 0.0..=5.0).step_by(1.0))
                        .on_hover_text("Number of retries for failed requests");
                });
                ui.horizontal(|ui| {
                    ui.label("Scrape Delay (seconds): ");
                    ui.add(Slider::new(&mut self.scrape_delay, 0.0..=5.0).step_by(0.1))
                        .on_hover_text("Delay between requests to avoid rate-limiting");
                });
                ui.horizontal(|ui| {
                    ui.label("Request Timeout (seconds): ");
                    ui.add(Slider::new(&mut self.timeout_secs, 1.0..=30.0).step_by(1.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Use Headless Browser: ");
                    ui.checkbox(&mut self.use_headless, "Enable")
                        .on_hover_text("Enable for JavaScript-heavy sites, disable for faster HTTP requests");
                });
            });

            // Action buttons
            ui.horizontal(|ui| {
                if ui.button("Scrape").clicked() {
                    // Update thread pool if max_concurrent changed
                    if (self.max_concurrent - self.last_max_concurrent).abs() > f32::EPSILON {
                        rayon::ThreadPoolBuilder::new()
                            .num_threads(self.max_concurrent as usize)
                            .build_global()
                            .unwrap_or_else(|e| {
                                self.status = format!("Failed to update thread pool: {}", e);
                                self.log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), self.status));
                                println!("Status: {}", self.status);
                            });
                        self.last_max_concurrent = self.max_concurrent;
                    }

                    let urls: Vec<String> = self.url_input
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let selector = self.selector_input.clone();
                    let attribute = if self.attribute_input.is_empty() {
                        None
                    } else {
                        Some(self.attribute_input.clone())
                    };
                    let regex = if self.regex_input.is_empty() {
                        None
                    } else {
                        Regex::new(&self.regex_input).ok()
                    };
                    let next_page_selector = self.next_page_selector.clone();
                    let content_type = self.content_type.clone();
                    let depth = self.crawl_depth as u32;
                    let headers = self.parse_headers();
                    let timeout = Duration::from_secs_f32(self.timeout_secs);
                    let proxy = self.proxy.clone();
                    let scrape_delay = self.scrape_delay;
                    let use_headless = self.use_headless;

                    if urls.is_empty() {
                        self.status = "No valid URLs provided".to_string();
                        self.log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), self.status));
                        println!("Status: {}", self.status);
                        ctx.request_repaint();
                    } else {
                        let results = Arc::clone(&self.results);
                        let log = Arc::clone(&self.log);
                        let progress = Arc::clone(&self.progress);
                        let total_urls = Arc::clone(&self.total_urls);
                        let tx = self.tx.clone();

                        // Spawn a thread for scraping
                        std::thread::spawn(move || {
                            let client = if proxy.is_empty() {
                                Client::builder()
                                    .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
                                    .timeout(timeout)
                                    .default_headers(headers)
                                    .build()
                                    .expect("Failed to build HTTP client")
                            } else {
                                let proxy = reqwest::Proxy::all(&proxy)
                                    .map_err(|e| format!("Invalid proxy {}: {}", proxy, e))
                                    .expect("Failed to set proxy");
                                Client::builder()
                                    .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
                                    .timeout(timeout)
                                    .default_headers(headers)
                                    .proxy(proxy)
                                    .build()
                                    .expect("Failed to build HTTP client")
                            };

                            let mut scraped_results = Vec::new();
                            let mut processed = 0;

                            let mut all_urls = HashSet::new();
                            let mut queue = VecDeque::new();
                            for url in urls {
                                let normalized = Self::normalize_url(&url);
                                all_urls.insert(normalized.clone());
                                queue.push_back((normalized, 0));
                            }
                            *total_urls.lock().unwrap() = all_urls.len().min(100);
                            *progress.lock().unwrap() = 0.0;
                            tx.send("Scraping...".to_string()).unwrap();

                            while let Some((current_url, current_depth)) = queue.pop_front() {
                                if current_depth > depth || all_urls.len() >= 100 {
                                    continue;
                                }

                                // Scrape current URL
                                match Self::scrape_url(&current_url, &client, &selector, &attribute, &regex, use_headless, &content_type) {
                                    Ok(data) => {
                                        scraped_results.push(data);
                                        let msg = format!("Scraped {}", current_url);
                                        println!("{}", msg);
                                        log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), msg));
                                        tx.send(msg).unwrap();
                                    }
                                    Err(e) => {
                                        let msg = format!("Error scraping {}: {}", current_url, e);
                                        println!("{}", msg);
                                        log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), msg));
                                        tx.send(msg).unwrap();
                                    }
                                }

                                // Update progress
                                processed += 1;
                                *progress.lock().unwrap() = processed as f32 / *total_urls.lock().unwrap() as f32;

                                // Apply scrape delay
                                if scrape_delay > 0.0 {
                                    std::thread::sleep(Duration::from_secs_f32(scrape_delay));
                                }

                                // Follow next page or crawl links
                                if !next_page_selector.is_empty() || current_depth < depth {
                                    if let Ok(html) = client.get(&current_url).send().and_then(|r| r.text()) {
                                        let document = Html::parse_document(&html);
                                        let base_url = Url::parse(&current_url).unwrap();
                                        let link_selector = if !next_page_selector.is_empty() {
                                            Selector::parse(&next_page_selector).unwrap_or_else(|_| Selector::parse("a").unwrap())
                                        } else {
                                            Selector::parse("a").unwrap()
                                        };
                                        for element in document.select(&link_selector) {
                                            if let Some(href) = element.value().attr("href") {
                                                if let Ok(absolute_url) = base_url.join(href) {
                                                    let url_str = absolute_url.to_string();
                                                    if !all_urls.contains(&url_str) && current_depth < depth {
                                                        all_urls.insert(url_str.clone());
                                                        queue.push_back((url_str, current_depth + 1));
                                                        *total_urls.lock().unwrap() = all_urls.len().min(100);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Deduplicate results
                            let mut unique_results = Vec::new();
                            let mut seen_content = HashSet::new();
                            for data in scraped_results {
                                if seen_content.insert(data.content.clone()) {
                                    unique_results.push(data);
                                }
                            }

                            let mut results_lock = results.lock().unwrap();
                            *results_lock = unique_results;
                            let msg = if results_lock.is_empty() {
                                "No successful results".to_string()
                            } else {
                                "Scraping completed".to_string()
                            };
                            println!("Status: {}", msg);
                            log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), msg));
                            tx.send(msg).unwrap();
                        });
                    }
                }

                if ui.button("Clear Results").clicked() {
                    self.results.lock().unwrap().clear();
                    self.log.lock().unwrap().clear();
                    self.status = "Results cleared".to_string();
                    *self.progress.lock().unwrap() = 0.0;
                    *self.total_urls.lock().unwrap() = 0;
                    println!("Status: {}", self.status);
                    ctx.request_repaint();
                }

                if ui.button("Save to CSV").clicked() {
                    if let Some(path) = FileDialog::new()
                        .add_filter("CSV", &["csv"])
                        .set_file_name("scraped_data.csv")
                        .save_file()
                    {
                        let filename = path.to_str().unwrap_or("scraped_data.csv");
                        match self.save_to_csv(filename) {
                            Ok(()) => {
                                self.status = format!("Saved to {}", filename);
                                self.log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), self.status));
                                println!("Status: {}", self.status);
                            }
                            Err(e) => {
                                self.status = format!("Failed to save: {}", e);
                                self.log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), self.status));
                                println!("Status: {}", self.status);
                            }
                        }
                        ctx.request_repaint();
                    }
                }

                if ui.button("Save to JSON").clicked() {
                    if let Some(path) = FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .set_file_name("scraped_data.json")
                        .save_file()
                    {
                        let filename = path.to_str().unwrap_or("scraped_data.json");
                        match self.save_to_json(filename) {
                            Ok(()) => {
                                self.status = format!("Saved to {}", filename);
                                self.log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), self.status));
                                println!("Status: {}", self.status);
                            }
                            Err(e) => {
                                self.status = format!("Failed to save: {}", e);
                                self.log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), self.status));
                                println!("Status: {}", self.status);
                            }
                        }
                        ctx.request_repaint();
                    }
                }
            });

            // Status with indicator
            ui.horizontal(|ui| {
                if self.status.contains("Scraping...") {
                    ui.spinner();
                }
                ui.label(RichText::new(&self.status).color(
                    if self.status.contains("Error") {
                        Color32::RED
                    } else if self.status.contains("Scraped") || self.status.contains("Saved") || self.status.contains("Loaded") {
                        Color32::GREEN
                    } else {
                        Color32::WHITE
                    }
                ));
            });

            // Progress bar
            let progress = *self.progress.lock().unwrap();
            if progress > 0.0 && progress < 1.0 {
                ui.add(ProgressBar::new(progress).show_percentage());
            }

            // Results
            ui.collapsing("Results", |ui| {
                ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    let results = self.results.lock().unwrap();
                    for data in results.iter() {
                        ui.label(format!("URL: {}", data.url));
                        ui.label(format!("Content: {}", data.content));
                        if !data.attributes.is_empty() {
                            ui.label(format!("Attributes: {}", data.attributes));
                        }
                        ui.separator();
                    }
                });
            });

            // Error log
            ui.collapsing("Error Log", |ui| {
                ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    let log = self.log.lock().unwrap();
                    for entry in log.iter() {
                        ui.label(entry);
                    }
                });
            });
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = NativeOptions {
        initial_window_size: Some(vec2(800.0, 600.0)),
        ..Default::default()
    };
    eframe::run_native(
        "Website Scraper",
        options,
        Box::new(|cc| Box::new(ScraperApp::new(cc))),
    )
}