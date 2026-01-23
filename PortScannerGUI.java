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

    // Common services map
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
        TCP_STEALTH("TCP Stealth"),
        UDP_SCAN("UDP Scan"),
        QUICK_SCAN("Quick Scan");

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

    private JPanel createControPanel() {
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

        
    }
}

