#![allow(unused_imports)] // Suppress unused-imports warnings

use eframe::{App, Frame, NativeOptions};
use eframe::egui::{CentralPanel, ScrollArea, vec2, Slider, TextEdit};
use reqwest::blocking::Client;
use scraper::{Html, Selector};
use url::Url;
use serde::Serialize;
use csv::Writer;
use rayon::prelude::*;
use crossbeam_channel::{bounded, Sender, Receiver};
use rfd::FileDialog;
use chrono::Local;
use std::sync::{Arc, Mutex};
use std::path::Path;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::time::Duration;
use regex::Regex;
use headless_chrome::{Browser, browser::LaunchOptions};
use std::collections::{HashSet, VecDeque};

#[derive(Serialize)]
struct ScrapedData {
    url: String,
    content: String,
    attributes: String,
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
    results: Arc<Mutex<Vec<ScrapedData>>>,
    status: String,
    log: Arc<Mutex<Vec<String>>>,
    progress: Arc<Mutex<f32>>,
    total_urls: Arc<Mutex<usize>>,
    tx: Sender<String>,
    rx: Receiver<String>,
}

impl ScraperApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
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
            results: Arc::new(Mutex::new(Vec::new())),
            status: "Ready to scrape".to_string(),
            log: Arc::new(Mutex::new(Vec::new())),
            progress: Arc::new(Mutex::new(0.0)),
            total_urls: Arc::new(Mutex::new(0)),
            tx,
            rx,
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
    ) -> Result<ScrapedData, String> {
        let normalized_url = Self::normalize_url(url);
        let parsed_url = Url::parse(&normalized_url).map_err(|e| format!("Invalid URL {}: {}", normalized_url, e))?;

        // Fetch HTML
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
                .map_err(|e| format!("Failed to get content for {}: {}", normalized_url, e.to_string()))?
        } else {
            let response = client
                .get(parsed_url.as_str())
                .send()
                .map_err(|e| format!("Failed to fetch {}: {}", normalized_url, e))?;
            response.text().map_err(|e| format!("Failed to read response from {}: {}", normalized_url, e))?
        };

        // Parse HTML
        let document = Html::parse_document(&html);
        let selector = Selector::parse(selector).map_err(|e| format!("Invalid selector: {:?}", e))?;

        // Extract text content
        let mut content = document
            .select(&selector)
            .map(|element| element.text().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join("\n");

        // Apply regex filtering
        if let Some(regex) = regex {
            let matches = regex
                .find_iter(&content)
                .map(|m| m.as_str().to_string())
                .collect::<Vec<_>>();
            content = if matches.is_empty() {
                return Err(format!("No regex matches found for {}", normalized_url));
            } else {
                matches.join("\n")
            };
        }

        // Extract attributes
        let attributes = if let Some(attr) = attribute {
            document
                .select(&selector)
                .filter_map(|element| element.value().attr(attr))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            "".to_string()
        };

        if content.is_empty() && attributes.is_empty() {
            return Err(format!("No content or attributes found for {}", normalized_url));
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
        while let Ok(status) = self.rx.try_recv() {
            self.status = status.clone();
            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            self.log.lock().unwrap().push(format!("[{}] {}", timestamp, status));
            println!("Status: {}", self.status);
            ctx.request_repaint();
        }

        CentralPanel::default().show(ctx, |ui| {
            ui.heading("Website Scraper");

            // URL input
            ui.horizontal(|ui| {
                ui.label("Enter URLs (comma-separated): ");
                ui.text_edit_singleline(&mut self.url_input);
            });

            // File picker for URLs
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
                            println!("Status: {}", self.status);
                        }
                        Err(e) => {
                            self.status = e;
                            self.log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), self.status));
                            println!("Status: {}", self.status);
                        }
                    }
                    ctx.request_repaint();
                }
            }

            // Selector input
            ui.horizontal(|ui| {
                ui.label("CSS Selector (e.g., p, h1, div.my-class): ");
                ui.text_edit_singleline(&mut self.selector_input);
            });

            // Attribute input
            ui.horizontal(|ui| {
                ui.label("HTML Attribute (e.g., href, src, leave blank for none): ");
                ui.text_edit_singleline(&mut self.attribute_input);
            });

            // Regex input
            ui.horizontal(|ui| {
                ui.label("Regex Filter (e.g., [a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}, leave blank for none): ");
                ui.text_edit_singleline(&mut self.regex_input);
            });

            // Next page selector
            ui.horizontal(|ui| {
                ui.label("Next Page Selector (e.g., a.next, leave blank for none): ");
                ui.text_edit_singleline(&mut self.next_page_selector);
            });

            // Crawl depth
            ui.horizontal(|ui| {
                ui.label("Crawl Depth (0 = no crawling): ");
                ui.add(Slider::new(&mut self.crawl_depth, 0.0..=5.0).step_by(1.0));
            });

            // Custom headers
            ui.horizontal(|ui| {
                ui.label("Custom Headers (key:value, one per line, e.g., Cookie: key=value): ");
                ui.add(TextEdit::multiline(&mut self.custom_headers).desired_rows(3));
            });

            // Timeout slider
            ui.horizontal(|ui| {
                ui.label("Request Timeout (seconds): ");
                ui.add(Slider::new(&mut self.timeout_secs, 1.0..=30.0).step_by(1.0));
            });

            // Action buttons
            ui.horizontal(|ui| {
                // Scrape button
                if ui.button("Scrape").clicked() {
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
                    let depth = self.crawl_depth as u32;
                    let headers = self.parse_headers();
                    let timeout = Duration::from_secs_f32(self.timeout_secs);
                    let use_headless = true;

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

                        // Crawl URLs with depth
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
                        ctx.request_repaint();

                        // Spawn a thread for scraping
                        std::thread::spawn(move || {
                            let client = Client::builder()
                                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
                                .timeout(timeout)
                                .default_headers(headers)
                                .build()
                                .expect("Failed to build HTTP client");

                            let mut scraped_results = Vec::new();
                            let mut processed = 0;

                            while let Some((current_url, current_depth)) = queue.pop_front() {
                                if current_depth > depth || all_urls.len() >= 100 {
                                    continue;
                                }

                                // Scrape current URL
                                match Self::scrape_url(&current_url, &client, &selector, &attribute, &regex, use_headless) {
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

                // Clear results button
                if ui.button("Clear Results").clicked() {
                    self.results.lock().unwrap().clear();
                    self.log.lock().unwrap().clear();
                    self.status = "Results cleared".to_string();
                    *self.progress.lock().unwrap() = 0.0;
                    *self.total_urls.lock().unwrap() = 0;
                    println!("Status: {}", self.status);
                    ctx.request_repaint();
                }

                // Save to CSV button
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
            });

            // Display status
            ui.label(&self.status);

            // Progress bar
            let progress = *self.progress.lock().unwrap();
            if progress > 0.0 && progress < 1.0 {
                ui.add(egui::ProgressBar::new(progress).show_percentage());
            }

            // Display results
            ui.label("Results:");
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

            // Error log window
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