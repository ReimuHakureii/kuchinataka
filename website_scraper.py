import tkinter as tk
from tkinter import ttk, filedialog
import requests
from bs4 import BeautifulSoup
from selenium import webdriver
from selenium.webdriver.chrome.options import Options
from urllib.parse import urljoin, urlparse
import re
import json
import csv
import os
from datetime import datetime
from multiprocessing import Pool, Manager, Queue
import time
import threading
from queue import Empty
from collections import OrderedDict
import logging

class ScrapedData:
    def __init__(self, url, content, attributes):
        self.url = url
        self.content = content
        self.attributes = attributes

    def to_dict(self):
        return {"url": self.url, "content": self.content, "attributes": self.attributes}
    
class ScraperConfig:
    def __init__(self):
        self.url_input = ""
        self.selector_input = "p,h1,h2,h3"
        self.attribute_input = ""
        self.regex_input = ""
        self.timeout_secs = 10.0
        self.crawl_depth = 1
        self.next_page_selector = ""
        self.custom_headers = ""
        self.proxy = ""
        self.max_concurrent = 4
        self.content_type = "text"
        self.retry_attempts = 2
        self.scrape_delay = 1.0
        self.use_headless = True

class ScraperApp:
    def __init__(self, root):
        self.root = root
        self.root.title("Website Scraper")
        self.root.geometry("800x600")

        # Init shared state
        self.manager = Manager()
        self.results = self.manager.list()
        self.log = self.manager.list()
        self.progress = self.manager.Value('f', 0.0)
        self.total_urls = self.manager.Value('i', 0)
        self.status_queue = Queue()

        # Init Config
        self.config = ScraperConfig()
        self.dark_mode = True
        self.last_max_concurrent = 4

        # Setup GUI
        self.setup_gui()
        self.setup_logging()

        # Start status update thread
        self.running = True
        self.status_thread = threading.Thread(target=self.update_status)
        self.status_thread_daemon = True
        self.status_thread.start()

    def setup_logging(self):
        logging.basicConfig(level=logging.INFO, format="%(asctime)s - %(message)s")
        self.logger = logging.getLogger()

    def setup_gui(self):
        # Main frame
        self.main_frame = ttk.Frame(self.root, padding="10")
        self.main_frame.grid(row=0, column=0, sticky="nsew")
        self.root.columnconfigure(0, weight=1)
        self.root.rowconfigure(0, weight=1)

        # Top panel (theme and config buttons)
        self.top_frame = ttk.Frame(self.main_frame)
        self.top_frame.grid(row=0, column=0, sticky="ew", pady=5)
        ttk.Label(self.top_frame, text="Website Scraper", font=("Arial", 16, "bold")).pack(side="left")
        ttk.Button(self.top_frame, text="Toggle Theme", command=self.toggle_theme).pack(side="left", padx=5)
        ttk.Button(self.top_frame, text="Save Config", command=self.save_config).pack(side="left", padx=5)
        ttk.Button(self.top_frame, text="Load Config", command=self.load_config).pack(side="left", padx=5)

        # Input settings
        self.input_frame = ttk.LabelFrame(self.main_frame, text="Input Settings", padding="5")
        self.input_frame.grid(row=1, column=0, sticky="ew", pady=5)
        ttk.Label(self.input_frame, text="URLs (comma-separated):").grid(row=0, column=0, sticky="w")
        self.url_input = ttk.Entry(self.input_frame, width=50)
        self.url_input.grid(row=0, column=1, sticky="ew", padx=5)
        ttk.Button(self.input_frame, text="Load URLs from File", command=self.load_urls_from_file).grid(row=1, column=0, columnspan=2, pady=5)

        # Scraping settings
        self.scrape_frame = ttk.LabelFrame(self.main_frame, text="Scraping Settings", padding="5")
        self.scrape_frame.grid(row=2, column=0, sticky="ew", pady=5)

        # Content Type
        ttk.Label(self.scrape_frame, text="Content Type:").grid(row=0, column=0, sticky="w")
        self.content_type_var = tk.StringVar(value=self.config.content_type)
        ttk.Combobox(self.scrape_frame, textvariable=self.content_type_var, values=["text", "links", "images"], state="readonly").grid(row=0, column=1, sticky="ew", padx=5)
        self.content_type_var.trace("w", self.update_content_type)

        # CSS Selector
        ttk.Label(self.scrape_frame, text="CSS Selector:").grid(row=1, column=0, sticky="w")
        self.selector_input = ttk.Entry(self.scrape_frame, width=50)
        self.selector_input.insert(0, self.config.selector_input)
        self.selector_input.grid(row=1, column=1, sticky="ew", padx=5)

        # HTML Attribute
        ttk.Label(self.scrape_frame, text="HTML Attribute:").grid(row=2, column=0, sticky="w")
        self.attribute_input = ttk.Entry(self.scrape_frame, width=50)
        self.attribute_input.insert(0, self.config.attribute_input)
        self.attribute_input.grid(row=2, column=1, sticky="ew", padx=5)

        # Regex Filter
        ttk.Label(self.scrape_frame, text="Regex Filter:").grid(row=3, column=0, sticky="w")
        self.regex_input = ttk.Entry(self.scrape_frame, width=50)
        self.regex_input.insert(0, self.config.regex_input)
        self.regex_input.grid(row=3, column=1, sticky="ew", padx=5)

        # Next Page Selector
        ttk.Label(self.scrape_frame, text="Next Page Selector:").grid(row=4, column=0, sticky="w")
        self.next_page_selector = ttk.Entry(self.scrape_frame, width=50)
        self.next_page_selector.insert(0, self.config.next_page_selector)
        self.next_page_selector.grid(row=4, column=1, sticky="ew", padx=5)

        # Crawl Depth
        ttk.Label(self.scrape_frame, text="Crawl Depth:").grid(row=5, column=0, sticky="w")
        self.crawl_depth = tk.DoubleVar(value=self.config.crawl_depth)
        ttk.Scale(self.scrape_frame, from_=0, to=5, orient="horizontal", variable=self.crawl_depth).grid(row=5, column=1, sticky="ew", padx=5)

        # Custom Headers
        ttk.Label(self.scrape_frame, text="Custom Headers:").grid(row=6, column=0, sticky="w")
        self.custom_headers = tk.Text(self.scrape_frame, height=3, width=50)
        self.custom_headers.grid(row=6, column=1, sticky="ew", padx=5)

        # Proxy
        ttk.Label(self.scrape_frame, text="Proxy (e.g., socks5://user:pass@host:port):").grid(row=7, column=0, sticky="w")
        self.proxy = ttk.Entry(self.scrape_frame, width=50)
        self.proxy.insert(0, self.config.proxy)
        self.proxy.grid(row=7, column=1, sticky="ew", padx=5)

        # Max Concurrent Requests
        ttk.Label(self.scrape_frame, text="Max Concurrent Requests:").grid(row=8, column=0, sticky="w")
        self.max_concurrent = tk.DoubleVar(value=self.config.max_concurrent)
        ttk.Scale(self.scrape_frame, from_=1, to=10, orient="horizontal", variable=self.max_concurrent).grid(row=8, column=1, sticky="ew", padx=5)

        # Retry Attempts
        ttk.Label(self.scrape_frame, text="Retry Attempts:").grid(row=9, column=0, sticky="w")
        self.retry_attempts = tk.DoubleVar(value=self.config.retry_attempts)
        ttk.Scale(self.scrape_frame, from_=0, to=5, orient="horizontal", variable=self.retry_attempts).grid(row=9, column=1, sticky="ew", padx=5)

        # Scrape Delay
        ttk.Label(self.scrape_frame, text="Scrape Delay (seconds):").grid(row=10, column=0, sticky="w")
        self.scrape_delay = tk.DoubleVar(value=self.config.scrape_delay)
        ttk.Scale(self.scrape_frame, from_=0, to=5, orient="horizontal", variable=self.scrape_delay).grid(row=10, column=1, sticky="ew", padx=5)

        # Request Timeout
        ttk.Label(self.scrape_frame, text="Request Timeout (seconds):").grid(row=11, column=0, sticky="w")
        self.timeout_secs = tk.DoubleVar(value=self.config.timeout_secs)
        ttk.Scale(self.scrape_frame, from_=1, to=30, orient="horizontal", variable=self.timeout_secs).grid(row=11, column=1, sticky="ew", padx=5)

        # Headless Browser
        ttk.Label(self.scrape_frame, text="Use Headless Browser:").grid(row=12, column=0, sticky="w")
        self.use_headless = tk.BooleanVar(value=self.config.use_headless)
        ttk.Checkbutton(self.scrape_frame, variable=self.use_headless).grid(row=12, column=1, sticky="w")

        # Action buttons
        self.action_frame = ttk.Frame(self.main_frame)
        self.action_frame.grid(row=3, column=0, sticky="ew", pady=5)
        ttk.Button(self.action_frame, text="Scrape", command=self.scrape).pack(side="left", padx=5)
        ttk.Button(self.action_frame, text="Clear Results", command=self.clear_results).pack(side="left", padx=5)
        ttk.Button(self.action_frame, text="Save to CSV", command=self.save_to_csv).pack(side="left", padx=5)
        ttk.Button(self.action_frame, text="Save to JSON", command=self.save_to_json).pack(side="left", padx=5)

        # Status and progress
        self.status_frame = ttk.Frame(self.main_frame)
        self.status_frame.grid(row=4, column=0, sticky="ew", pady=5)
        self.status_label = ttk.Label(self.status_frame, text="Ready to scrape")
        self.status_label.pack(side="left")
        self.progress_bar = ttk.Progressbar(self.status_frame, maximum=1.0)
        self.progress_bar.pack(side="left", fill="x", expand=True, padx=5)

        # Results
        self.results_frame = ttk.LabelFrame(self.main_frame, text="Results", padding="5")
        self.results_frame.grid(row=5, column=0, sticky="nsew", pady=5)
        self.results_text = tk.Text(self.results_frame, height=10, width=80)
        self.results_text.grid(row=0, column=0, sticky="nsew")
        self.results_scroll = ttk.Scrollbar(self.results_frame, orient="vertical", command=self.results_text.yview)
        self.results_scroll.grid(row=0, column=1, sticky="ns")
        self.results_text.config(yscrollcommand=self.results_scroll.set)

        # Error log
        self.log_frame = ttk.LabelFrame(self.main_frame, text="Error Log", padding="5")
        self.log_frame.grid(row=6, column=0, sticky="nsew", pady=5)
        self.log_text = tk.Text(self.log_frame, height=10, width=80)
        self.log_text.grid(row=0, column=0, sticky="nsew")
        self.log_scroll = ttk.Scrollbar(self.log_frame, orient="vertical", command=self.log_text.yview)
        self.log_scroll.grid(row=0, column=1, sticky="ns")
        self.log_text.config(yscrollcommand=self.log_scroll.set)

        # Configure grid weights
        self.main_frame.columnconfigure(0, weight=1)
        self.main_frame.rowconfigure(5, weight=1)
        self.main_frame.rowconfigure(6, weight=1)
        self.results_frame.columnconfigure(0, weight=1)
        self.results_frame.rowconfigure(0, weight=1)
        self.log_frame.columnconfigure(0, weight=1)
        self.log_frame.rowconfigure(0, weight=1)

    def toggle_theme(self):
        self.dark_mode = not self.dark_mode
        # Tkinter doesn't support dynamic theme switching easily, so this is a placeholder
        self.status_label.config(text="Theme toggled (not fully implemented in tkinter)")

    def update_content_type(self, *args):
        content_type = self.content_type_var.get()
        if content_type == "text":
            self.selector_input.delete(0, tk.END)
            self.selector_input.insert(0, "p,h1,h2,h3")
            self.attribute_input.delete(0, tk.END)
        elif content_type == "links":
            self.selector_input.delete(0, tk.END)
            self.selector_input.insert(0, "a")
            self.attribute_input.delete(0, tk.END)
            self.attribute_input.insert(0, "href")
        elif content_type == "images":
            self.selector_input.delete(0, tk.END)
            self.selector_input.insert(0, "img")
            self.attribute_input.delete(0, tk.END)
            self.attribute_input.insert(0, "src")

    def normalize_url(self, url):
        if not url.startswith(("http://", "https://")):
            return f"https://{url}"
        return url
    
    def scrape_url(self, args):
        url, selector, attribute, regex, use_headless, content_type, timeout, proxy, retry_attempts, status_queue = args
        normalized_url = self.normalize_url(url)
        attempts = 0
        while attempts <= retry_attempts:
            try:
                if use_headless:
                    chrome_options = Options()
                    chrome_options.add_argument("--headless")
                    driver = webdriver.Chrome(options=chrome_options)
                    try:
                        driver.get(normalized_url)
                        driver.implicitly_wait(timeout)
                        html = driver.page_source
                    finally:
                        driver.quit()
                else:
                    proxies = {"http": proxy, "https": proxy} if proxy else None
                    response = requests.get(normalized_url, timeout=timeout, proxies=proxies)
                    response.raise_for_status()
                    html = response.text
                
                soup = BeautifulSoup(html, "html.parser")
                elements = soup.select(selector)
                if not elements:
                    raise Exception(f"No elements found for selector '{selector}' on {normalized_url}")
                
                if content_type.lower() == "text":
                    content = "\n".join(" ".join(element.get_text(strip=True).split()) for element in elements)
                elif content_type.lower() == "links":
                    content = "\n".join(element.get("href", "") for element in elements)
                elif content_type.lower() == "images":
                    content = "\n".join(element.get("src", "") for element in elements)
                else:
                    raise Exception(f"Invalid content type: {content_type}")
                
                if regex:
                    matches = regex.findall(content)
                    if not matches:
                        raise Exception(f"No regex matches found for {regex.pattern} on {normalized_url}")
                    content = "\n".join(matches)

                attributes = ""
                if content_type.lower() == "text" and attribute:
                    attributes = "\n".join(element.get(attribute, "") for element in elements)

                if not content:
                    raise Exception(f"No content extracted for {content_type} on {normalized_url}")
                
                status_queue.put(f"Scraped {normalized_url}")
                return ScrapedData(normalized_url, content, attributes)
            except Exception as e:
                attempts += 1
                if attempts > retry_attempts:
                    error_msg = f"Error scraping {normalized_url}: {str(e)}"
                    status_queue.put(error_msg)
                    self.logger.error(error_msg)
                    return None
                time.sleep(1)
    
    def save_to_csv(self):
        filename = filedialog.asksaveasfilename(defaultextension=".csv", filetypes=[("CSV files", "*.csv")])
        if not filename:
            return
        if not self.results:
            self.status.queue.put("No data to save")
            return
        try:
            with open(filename, "w", newline="", encoding="utf-8") as f:
                writer = csv.DictWriter(f, fieldnames=["url", "content", "attributes"])
                writer.writeheader()
                for data in self.results:
                    writer.writerow(data.to_dict())
            self.status_queue.put(f"Saved to {filename}")
        except Exception as e:
            self.status_queue.put(f"Failed to save CSV: {str(e)}")

    def save_to_json(self):
        filename = filedialog.asksaveasfilename(defaultextension=".json", filetypes=[("JSON files", "*json")])
        if not filename:
            return
        if not self.results:
            self.status_queue.put("No data to save")
            return
        try:
            with open(filename, "w", encoding="utf-8") as f:
                json.dump([data.to_dict() for data in self.results], f, indent=2)
            self.status_queue.put(f"Saved to {filename}")
        except Exception as e:
            self.status_queue.put(f"Failed to save JSON: {str(e)}")

    def load_urls_from_file(self):
        filename = filedialog.askopenfilename(filetypes=[("Text files", "*.txt")])
        if not filename:
            return
        try:
            with open(filename, "r", encoding="utf-8") as f:
                urls = [line.strip() for line in f if line.strip()]
            if not urls:
                self.status_queue.put("No valid URLS found in file")
                return
            self.url_input.delete(0, tk.END)
            self.url_input.insert(0, ", ".join(urls))
            self.status_queue.put(f"Loaded {len(urls)} URLS from file")
        except Exception as e:
            self.status_queue.put(f"Failed to load URLS: {str(e)}")
    
    def save_config(self):
        filename = filedialog.asksaveasfilename(defaultextension=".json", filetypes=[("JSON files", "*.json")])
        if not filename:
            return
        config = {
            "url_input": self.url_input.get(),
            "selector_input": self.selector_input.get(),
            "regex_input": self.regex_input.get(),
            "timeout_secs": self.timeout_secs.get(),
            "crawl_depth": self.crawl_depth.get(),
            "next_page_selector": self.next_page_selector.get(),
            "custom_headers": self.custom_headers.get("1.0", tk.END).strip(),
            "proxy": self.proxy.get(),
            "max_concurrent": self.max_concurrent.get(),
            "content_type": self.content_type_var.get(),
            "retry_attempts": self.retry_attempts.get(),
            "scrape_delay": self.scrape_delay.get(),
            "use_headless": self.use_headless.get(),
        }
        try:
            with open(filename, "w", encoding="utf-8") as f:
                json.dump(config, f, indent=2)
            self.status_queue.put(f"Failed to save to {filename}")
        except Exception as e:
            self.status_queue.put(f"Failed to load config: {str(e)}")
    
    def parse_headers(self):
        headers = {}
        for line in self.custom_headers.get("1.0", tk.END).strip().split("\n"):
            parts = line.split(":", 1)
            if len(parts) == 2:
                headers[parts[0].strip()] = parts[1].strip()
        return headers
    
    def update_status(self):
        while self.running:
            try:
                status = self.status_queue.get_nowait()
                self.status_label.config(text=status)
                timestamp = datetime.now().strftime("%Y-%m-%d %H: %M: %S")
                self.log.append(f"[{timestamp}] {status}")
                self.log_text.delete("1.0", tk.END)
                self.log_text.insert("1.0", "\n".join(self.log))
                self.logger.info(status)
                self.update_results()
                if "Error" in status:
                    self.status_label.config(foreground="red")
                elif "Scraped" in status or "Saved" in status or "Loaded" in status:
                    self.status_label.config(foreground="green")
                else:
                    self.status_label.config(foreground="black" if self.dark_mode else "black")
            except Empty:
                pass
            self.progress_bar["value"] = self.progress.value
            self.root.update()
            time.sleep(0.1)
    
    def update_results(self):
        self.results_text.delete("1.0", tk.END)
        for data in self.results:
            self.results_text.insert(tk.END, f"URL: {data.url}\n")
            self.results_text.insert(tk.END, f"Content: {data.content}\n")
            if data.attributes:
                self.results_text.insert(tk.END, f"Attributes: {data.attributes}\n")
            self.results_text.insert(tk.END, "-" * 50 + "\n")

    def scrape(self):
        urls = [url.strip() for url in self.url_input.get().split(",") if url.strip()]
        if not urls:
            self.status_queue.put("No valid URLs provided")
            return

        if self.max_concurrent.get() != self.last_max_concurrent:
            self.last_max_concurrent = self.max_concurrent.get()

        selector = self.selector_input.get()
        attribute = self.attribute_input.get()
        regex = re.compile(self.regex_input.get())
        next_page_selector = self.next_page_selector.get()
        content_type = self.content_type_var.get()
        depth = int(self.crawl_depth.get())
        headers = self.parse_headers()
        timeout = self.timeout_secs.get()
        proxy = self.proxy.get()
        scrape_delay = self.scrape_delay.get()
        use_headless = self.use_headless.get()
        retry_attempts = int(self.retry_attempts.get())

        self.results[:] = []
        self.log[:] = []
        self.progress.value = 0.0
        self.total_urls.value = 0
        self.status_queue.put("Scraping...")

        def scrape_worker():
            all_urls = set()
            queue = [(self.normalize_url(url), 0) for url in urls]
            seen_content = set()
            scraped_results = []

            self.total_urls.value = min(len(queue), 100)

            with Pool(processes=int(self.max_concurrent.get())) as pool:
                while queue:
                    current_url, current_depth = queue.pop(0)
                    if current_depth > depth or len(all_urls) >= 100:
                        continue

                    all_urls.add(current_url)
                    args = (current_url, selector, attribute, regex, use_headless, content_type, timeout, proxy, retry_attempts, self.status_queue)
                    result = pool.apply(self.scrape_url, (args,))
                    if result:
                        if result.content not in seen_content:
                            scraped_results.append(result)
                            seen_content.add(result.content)

                    self.progress.value = len(scraped_results) / self.total_urls.value

                    if scrape_delay > 0:
                        time.sleep(scrape_delay)
                    
                    if next_page_selector or current_depth < depth:
                        try:
                            response = requests.get(current_url, headers=headers, timeout=timeout, proxies={"http": proxy, "https": proxy} if proxy else None)
                            soup = BeautifulSoup(response.text, "html_parser")
                            link_selector = next_page_selector or "a"
                            for element in soup.select(link_selector):
                                href = element.get("href")
                                if href:
                                    absolute_url = urljoin(current_url, href)
                                    if absolute_url not in all_urls and current_depth < depth:
                                        all_urls.add(absolute_url)
                                        queue.append((absolute_url, current_depth + 1))
                                        self.total_urls.value = min(len(all_urls), 100)
                        except Exception as e:
                            self.status_queue.put(f"Error crawling links from {current_url}: {str(e)}")

            self.results.extend(scraped_results)
            self.status_queue.put("Scraping completed" if scraped_results else "No successful results")
        
        threading.Thread(target=scrape_worker, daemon=True).start()

    def clear_results(self):
        self.results[:] = []
        self.log[:] = []
        self.progress.value = 0.0
        self.total_urls.value = 0
        self.status_queue.put("Results cleared")
        self.results_text.delete("1.0", tk.END)
        self.log_text.delete("1.0", tk.END)

    def destroy(self):
        self.running = False
        self.status_thread.join()
        self.root.destroy()

def main():
    root = tk.Tk()
    app = ScraperApp(root)
    root.mainloop()

if __name__ == "__main__":
    main()   