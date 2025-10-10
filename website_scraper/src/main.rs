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

#[derive(Serialize)]
struct ScrapedData {
    url: String,
    content: String,
}

struct ScraperApp {
    url_input: String,
    selector_input: String,
    timeout_secs: f32,
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
        let (tx, rx) = bounded(100); // Channel for status updates
        Self {
            url_input: String::new(),
            selector_input: "p, h1, h2, h3".to_string(), // Default selector
            timeout_secs: 10.0, // Default timeout: 10 seconds
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

    fn scrape_url(url: &str, client: &Client, selector: &str) -> Result<ScrapedData, String> {
        let normalized_url = Self::normalize_url(url);
        let parsed_url = Url::parse(&normalized_url).map_err(|e| format!("Invalid URL {}: {}", normalized_url, e))?;

        let response = client
            .get(parsed_url.as_str())
            .send()
            .map_err(|e| format!("Failed to fetch {}: {}", normalized_url, e))?;
        
        let text = response
            .text()
            .map_err(|e| format!("Failed to read response from {}: {}", normalized_url, e))?;

        let document = Html::parse_document(&text);
        let selector = Selector::parse(selector).map_err(|e| format!("Invalid selector '{}': {:?}", selector, e))?;

        let content = document
            .select(&selector)
            .map(|element| element.text().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join("\n");

        if content.is_empty() {
            return Err(format!("No content found for {} with selector '{}'", normalized_url, selector));
        }

        Ok(ScrapedData {
            url: normalized_url,
            content,
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
}

impl App for ScraperApp {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut Frame) {
        // Check for status updates
        while let Ok(status) = self.rx.try_recv() {
            self.status = status.clone();
            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            self.log.lock().unwrap().push(format!("[{}] {}", timestamp, status));
            println!("Status: {}", self.status); // Log to console
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
                    let timeout = Duration::from_secs_f32(self.timeout_secs);

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
                        *total_urls.lock().unwrap() = urls.len();
                        *progress.lock().unwrap() = 0.0;
                        tx.send("Scraping...".to_string()).unwrap();
                        ctx.request_repaint();

                        // Spawn a thread for scraping
                        std::thread::spawn(move || {
                            let client = Client::builder()
                                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
                                .timeout(timeout)
                                .build()
                                .expect("Failed to build HTTP client");
                            let scraped_results: Vec<_> = urls
                                .par_iter()
                                .enumerate()
                                .map(|(i, url)| {
                                    let result = Self::scrape_url(url, &client, &selector);
                                    let msg = match &result {
                                        Ok(data) => format!("Scraped {}", url),
                                        Err(e) => format!("Error scraping {}: {}", url, e),
                                    };
                                    println!("{}", msg);
                                    log.lock().unwrap().push(format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), msg));
                                    *progress.lock().unwrap() = (i + 1) as f32 / urls.len() as f32;
                                    tx.send(msg).unwrap();
                                    result.ok()
                                })
                                .filter_map(|x| x)
                                .collect();

                            let mut results_lock = results.lock().unwrap();
                            *results_lock = scraped_results;
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
                    self.progress.lock().unwrap() = 0.0;
                    self.total_urls.lock().unwrap() = 0;
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