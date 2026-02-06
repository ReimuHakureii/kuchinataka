import javax.swing.*;
import javax.swing.border.*;
import javax.swing.table.DefaultTableModel;
import javax.swing.table.TableRowSorter;
import java.awt.*;
import java.awt.event.*;
import java.io.*;
import java.net.*;
import java.text.SimpleDateFormat;
import java.util.*;
import java.util.List;
import java.util.concurrent.*;

public class PortScannerGUI extends JFrame {
    private JTextField hostField;
    private JTextField startPortField;
    private JTextField endPortField;
    private JSpinner timeoutSpinner;
    private JSpinner threadSpinner;
    private JComboBox<ScanMode> scanModeCombo;
    private JButton scanButton;
    private JButton stopButton;
    private JButton clearButton;
    private JButton exportButton;
    private JProgressBar progressBar;
    private JLabel statusLabel;
    private JLabel statsLabel;
    private JTable resultsTable;
    private DefaultTableModel tableModel;
    private JTextArea logArea;

    private PortScanner scanner;
    private boolean isScanning = false;

    private static final Map<Integer, String> COMMON_SERVICES = new HashMap<>() {{
        put(20, "FTP Data");
        put(21, "FTP Control");
        put(22, "SSH");
        put(23, "Telnet");
        put(25, "SMTP");
        put(53, "DNS");
        put(80, "HTTP");
        put(110, "POP3");
        put(143, "IMAP");
        put(443, "HTTPS");
        put(445, "SMB");
        put(3306, "MySQL");
        put(3389, "RDP");
        put(5432, "PostgreSQL");
        put(5900, "VNC");
        put(6379, "Redis");
        put(8080, "HTTP-Proxy");
        put(8443, "HTTPS-Alt");
        put(27017, "MongoDB");
    }};

    public enum ScanMode {
        TCP_CONNECT("TCP Connect"),
        TCP_STEALTH("TCP Stealth (SYN)"),
        UDP_SCAN("UDP Scan"),
        QUICK_SCAN("Quick Scan (Common Ports)");

        private final String displayName;

        ScanMode(String displayName) {
            this.displayName = displayName;
        }

        @Override
        public String toString() {
            return displayName;
        }
    }

    public PortScannerGUI() {
        setTitle("Advanced Port Scanner");
        setDefaultCloseOperation(JFrame.EXIT_ON_CLOSE);
        setSize(1000, 700);
        setLocationRelativeTo(null);

        initComponents();
        setVisible(true);
    }

    private void initComponents() {
        JPanel mainPanel = new JPanel(new BorderLayout(10, 10));
        mainPanel.setBorder(new EmptyBorder(10, 10, 10, 10));

        mainPanel.add(createControlPanel(), BorderLayout.NORTH);
        mainPanel.add(createResultsPanel(), BorderLayout.CENTER);
        mainPanel.add(createStatusPanel(), BorderLayout.SOUTH);

        add(mainPanel);
    }

    private JPanel createControlPanel() {
        JPanel panel = new JPanel(new BorderLayout(10, 10));
        panel.setBorder(BorderFactory.createTitledBorder("Scan Configuration"));

        JPanel inputPanel = new JPanel(new GridBagLayout());
        GridBagConstraints gbc = new GridBagConstraints();
        gbc.fill = GridBagConstraints.HORIZONTAL;
        gbc.insets = new Insets(5, 5, 5, 5);

        gbc.gridx = 0; gbc.gridy = 0;
        inputPanel.add(new JLabel("Target Host:"), gbc);
        gbc.gridx = 1; gbc.weightx = 1.0;
        hostField = new JTextField("localhost");
        inputPanel.add(hostField, gbc);

        gbc.gridx = 0; gbc.gridy = 1; gbc.weightx = 0;
        inputPanel.add(new JLabel("Start Port:"), gbc);
        gbc.gridx = 1; gbc.weightx = 0.5;
        startPortField = new JTextField("1");
        inputPanel.add(startPortField, gbc);

        gbc.gridx = 2; gbc.weightx = 0;
        inputPanel.add(new JLabel("End Port:"), gbc);
        gbc.gridx = 3; gbc.weightx = 0.5;
        endPortField = new JTextField("1000");
        inputPanel.add(endPortField, gbc);

        gbc.gridx = 0; gbc.gridy = 2; gbc.weightx = 0;
        inputPanel.add(new JLabel("Timeout (ms):"), gbc);
        gbc.gridx = 1;
        timeoutSpinner = new JSpinner(new SpinnerNumberModel(1000, 100, 10000, 100));
        inputPanel.add(timeoutSpinner, gbc);

        gbc.gridx = 2;
        inputPanel.add(new JLabel("Threads:"), gbc);
        gbc.gridx = 3;
        threadSpinner = new JSpinner(new SpinnerNumberModel(50, 1, 500, 10));
        inputPanel.add(threadSpinner, gbc);

        gbc.gridx = 0; gbc.gridy = 3;
        inputPanel.add(new JLabel("Scan Mode:"), gbc);
        gbc.gridx = 1; gbc.gridwidth = 3;
        scanModeCombo = new JComboBox<>(ScanMode.values());
        inputPanel.add(scanModeCombo, gbc);

        JPanel buttonPanel = new JPanel(new FlowLayout(FlowLayout.CENTER, 10, 5));
        scanButton = new JButton("Start Scan");
        scanButton.setBackground(new Color(76, 175, 80));
        scanButton.setForeground(Color.WHITE);
        scanButton.setFocusPainted(false);
        scanButton.addActionListener(e -> startScan());

        stopButton = new JButton("Stop Scan");
        stopButton.setBackground(new Color(244, 67, 54));
        stopButton.setForeground(Color.WHITE);
        stopButton.setFocusPainted(false);
        stopButton.setEnabled(false);
        stopButton.addActionListener(e -> stopScan());

        clearButton = new JButton("Clear Results");
        clearButton.addActionListener(e -> clearResults());

        exportButton = new JButton("Export Results");
        exportButton.addActionListener(e -> exportResults());

        buttonPanel.add(scanButton);
        buttonPanel.add(stopButton);
        buttonPanel.add(clearButton);
        buttonPanel.add(exportButton);

        panel.add(inputPanel, BorderLayout.CENTER);
        panel.add(buttonPanel, BorderLayout.SOUTH);

        return panel;
    }

    private JPanel createResultsPanel() {
        JPanel panel = new JPanel(new BorderLayout(5, 5));

        JSplitPane splitPane = new JSplitPane(JSplitPane.VERTICAL_SPLIT);
        splitPane.setResizeWeight(0.7);

        JPanel tablePanel = new JPanel(new BorderLayout());
        tablePanel.setBorder(BorderFactory.createTitledBorder("Open Ports"));

        String[] columns = {"Port", "State", "Service", "Response Time (ms)"};
        tableModel = new DefaultTableModel(columns, 0) {
            @Override
            public boolean isCellEditable(int row, int column) {
                return false;
            }
        };
        resultsTable = new JTable(tableModel);
        resultsTable.setAutoCreateRowSorter(true);
        resultsTable.getColumnModel().getColumn(0).setPreferredWidth(80);
        resultsTable.getColumnModel().getColumn(1).setPreferredWidth(80);
        resultsTable.getColumnModel().getColumn(2).setPreferredWidth(150);
        resultsTable.getColumnModel().getColumn(3).setPreferredWidth(120);

        JScrollPane tableScroll = new JScrollPane(resultsTable);
        tablePanel.add(tableScroll, BorderLayout.CENTER);

        JPanel logPanel = new JPanel(new BorderLayout());
        logPanel.setBorder(BorderFactory.createTitledBorder("Scan Log"));

        logArea = new JTextArea();
        logArea.setEditable(false);
        logArea.setFont(new Font("Monospaced", Font.PLAIN, 11));
        JScrollPane logScroll = new JScrollPane(logArea);
        logPanel.add(logScroll, BorderLayout.CENTER);

        splitPane.setTopComponent(tablePanel);
        splitPane.setBottomComponent(logPanel);

        panel.add(splitPane, BorderLayout.CENTER);

        return panel;
    }

    private JPanel createStatusPanel() {
        JPanel panel = new JPanel(new BorderLayout(5, 5));
        panel.setBorder(new EmptyBorder(5, 0, 0, 0));

        progressBar = new JProgressBar();
        progressBar.setStringPainted(true);

        statusLabel = new JLabel("Ready to scan");
        statusLabel.setBorder(new EmptyBorder(5, 0, 5, 0));

        statsLabel = new JLabel("Open: 0 | Closed: 0 | Total: 0");
        statsLabel.setBorder(new EmptyBorder(5, 0, 5, 0));
        statsLabel.setHorizontalAlignment(SwingConstants.RIGHT);

        JPanel statusRow = new JPanel(new BorderLayout());
        statusRow.add(statusLabel, BorderLayout.WEST);
        statusRow.add(statsLabel, BorderLayout.EAST);

        panel.add(statusRow, BorderLayout.NORTH);
        panel.add(progressBar, BorderLayout.CENTER);

        return panel;
    }

    private void startScan() {
        if (isScanning) return;

        String host = hostField.getText().trim();
        if (host.isEmpty()) {
            JOptionPane.showMessageDialog(this, "Please enter a target host", "Error", JOptionPane.ERROR_MESSAGE);
            return;
        }

        int startPort, endPort;
        try {
            startPort = Integer.parseInt(startPortField.getText().trim());
            endPort = Integer.parseInt(endPortField.getText().trim());

            if (startPort < 1 || startPort > 65535 || endPort < 1 || endPort > 65535) {
                throw new NumberFormatException();
            }
            if (startPort > endPort) {
                throw new IllegalArgumentException("Start port must be less than or equal to end port");
            }
        } catch (Exception e) {
            JOptionPane.showMessageDialog(this, "Invalid port range (1-65535)", "Error", JOptionPane.ERROR_MESSAGE);
            return;
        }

        int timeout = (Integer) timeoutSpinner.getValue();
        int threads = (Integer) threadSpinner.getValue();
        ScanMode mode = (ScanMode) scanModeCombo.getSelectedItem();

        isScanning = true;
        scanButton.setEnabled(false);
        stopButton.setEnabled(true);
        progressBar.setValue(0);
        progressBar.setMaximum(endPort - startPort + 1);

        log("Starting scan on " + host + " (ports " + startPort + "-" + endPort + ")");
        log("Mode: " + mode + ", Timeout: " + timeout + "ms, Threads: " + threads);

        scanner = new PortScanner(host, startPort, endPort, timeout, threads, mode);
        scanner.execute();
    }

    private void stopScan() {
        if (scanner != null && !scanner.isDone()) {
            scanner.cancel(true);
            log("Scan stopped by user");
            statusLabel.setText("Scan stopped");
            isScanning = false;
            scanButton.setEnabled(true);
            stopButton.setEnabled(false);
        }
    }

    private void clearResults() {
        tableModel.setRowCount(0);
        logArea.setText("");
        progressBar.setValue(0);
        statusLabel.setText("Ready to scan");
        statsLabel.setText("Open: 0 | Closed: 0 | Total: 0");
    }

    private void exportResults() {
        if (tableModel.getRowCount() == 0) {
            JOptionPane.showMessageDialog(this, "No results to export", "Info", JOptionPane.INFORMATION_MESSAGE);
            return;
        }

        JFileChooser fileChooser = new JFileChooser();
        fileChooser.setDialogTitle("Export Results");
        fileChooser.setSelectedFile(new File("scan_results_" + System.currentTimeMillis() + ".csv"));

        if (fileChooser.showSaveDialog(this) == JFileChooser.APPROVE_OPTION) {
            try (PrintWriter writer = new PrintWriter(fileChooser.getSelectedFile())) {
                writer.println("Port,State,Service,Response Time (ms)");
                for (int i = 0; i < tableModel.getRowCount(); i++) {
                    writer.println(tableModel.getValueAt(i, 0) + "," + tableModel.getValueAt(i, 1) + "," + tableModel.getValueAt(i, 2) + "," + tableModel.getValueAt(i, 3));
                }

                log("Results exported to " + fileChooser.getSelectedFile().getName());
                JOptionPane.showMessageDialog(this, "Results exported successfully", "Success", JOptionPane.INFORMATION_MESSAGE);
            } catch (IOException e) {
                JOptionPane.showMessageDialog(this, "Error exporting results: " + e.getMessage(), "Error", JOptionPane.ERROR_MESSAGE);
            }
        }
    }

    private void log(String message) {
        String timestamp = new SimpleDateFormat("HH:mm:ss").format(new Date());
        logArea.append("[" + timestamp + "] " + message + "\n");
        logArea.setCaretPosition(logArea.getDocument().getLength());
    }

    private class PortScanner extends SwingWorker<Void, PortResult> {
        private final String host;
        private final int startPort;
        private final int endPort;
        private final int timeout;
        private final int threadCount;
        private final ScanMode mode;
        private final ExecutorService executor;
        private int openPorts = 0;
        private int closedPorts = 0;
        private int scannedPorts = 0;

        public PortScanner(String host, int startPort, int endPort, int timeout, int threadCount, ScanMode mode) {
            this.host = host;
            this.startPort = startPort;
            this.endPort = endPort;
            this.timeout = timeout;
            this.mode = mode;
            this.executor = Executors.newFixedThreadPool(threadCount); 
        }

        @Override
        protected Void doInBackground() throws Exception {
            List<Future<PortResult>> futures = new ArrayList<>();

            for (int port = startPort; port <= endPort; port++) {
                if (isCancelled()) break;
                final int currentPort = port;
                Future<PortResult> future = executor.submit(() -> scanPort(currentPort));
                futures.add(future);
            }

            for (Future<PortResult> future : futures) {
                if (isCancelled()) break;
                try {
                    PortResult result = future.get();
                    publish(result);
                } catch (Exception e) {
                    // Handle Exception
                }
            }

            executor.shutdown();
            return null;
        }

        private PortResult scanPort(int port) {
            long startTime = System.currentTimeMillis();
            boolean isOpen = false;
            String state = "Closed";

            try {
                if (mode == ScanMode.QUICK_SCAN && !COMMON_SERVICES.containsKey(port)) {
                    return new PortResult(port, false, "Skipped", 0);
                }

                Socket socket = new Socket();
                socket.connect(new InetSocketAddress(host, port), timeout);
                socket.close();
                isOpen = true;
                state = "Open";
            } catch (IOException e) {
                state = "Closed";
            }

            long responseTime = System.currentTimeMillis() - startTime;
            return new PortResult(port, isOpen, state, responseTime);
        }

        @Override
        protected void process(List<PortResult> results) {
            for (PortResult result : results) {
                scannedPorts++;
                progressBar.setValue(scannedPorts);

                if (result.isOpen) {
                    
                }
            }
        }
    }
}