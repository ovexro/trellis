#include "TrellisProvisioning.h"

// Minimal captive portal HTML — clean, dark, mobile-friendly
static const char PROVISIONING_HTML[] PROGMEM = R"rawliteral(
<!DOCTYPE html>
<html><head>
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Trellis Setup</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:system-ui;background:#09090b;color:#e4e4e7;display:flex;justify-content:center;padding:20px}
.c{max-width:400px;width:100%}
h1{font-size:1.5em;color:#4ade80;margin-bottom:4px}
p{font-size:.85em;color:#71717a;margin-bottom:20px}
label{display:block;font-size:.8em;color:#a1a1aa;margin-bottom:4px}
input,select{width:100%;padding:10px;background:#18181b;border:1px solid #27272a;border-radius:8px;color:#e4e4e7;font-size:.9em;margin-bottom:12px}
input:focus{border-color:#4ade80;outline:none}
button{width:100%;padding:12px;background:#22c55e;border:none;border-radius:8px;color:#fff;font-size:.95em;font-weight:600;cursor:pointer}
button:hover{background:#16a34a}
.s{font-size:.75em;color:#52525b;margin-top:16px;text-align:center}
#networks{margin-bottom:12px}
.net{padding:8px 12px;background:#18181b;border:1px solid #27272a;border-radius:6px;margin-bottom:4px;cursor:pointer;font-size:.85em;display:flex;justify-content:space-between}
.net:hover{border-color:#4ade80}
.net .rssi{color:#71717a;font-size:.8em}
</style>
</head><body>
<div class="c">
<h1>Trellis Setup</h1>
<p>Connect this device to your WiFi network.</p>
<div id="networks"><div class="net" style="color:#71717a">Scanning networks...</div></div>
<form action="/configure" method="POST">
<label>WiFi Network</label>
<input type="text" name="ssid" id="ssid" placeholder="Network name" required>
<label>Password</label>
<input type="password" name="password" placeholder="WiFi password" required>
<button type="submit">Connect</button>
</form>
<p class="s">Powered by Trellis &mdash; github.com/ovexro/trellis</p>
</div>
<script>
fetch('/scan').then(r=>r.json()).then(nets=>{
  const c=document.getElementById('networks');
  c.innerHTML='';
  if(!nets.length){c.innerHTML='<div class="net" style="color:#71717a">No networks found</div>';return;}
  nets.forEach(n=>{
    const d=document.createElement('div');d.className='net';
    const s=document.createElement('span');s.textContent=n.ssid;
    const r=document.createElement('span');r.className='rssi';r.textContent=n.rssi+' dBm';
    d.appendChild(s);d.appendChild(r);
    d.onclick=()=>{document.getElementById('ssid').value=n.ssid;};
    c.appendChild(d);
  });
});
</script>
</body></html>
)rawliteral";

static const char PROVISIONING_SUCCESS[] PROGMEM = R"rawliteral(
<!DOCTYPE html>
<html><head>
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Trellis Setup</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:system-ui;background:#09090b;color:#e4e4e7;display:flex;justify-content:center;align-items:center;min-height:100vh;padding:20px}
.c{text-align:center}
h1{font-size:1.5em;color:#4ade80;margin-bottom:8px}
p{font-size:.9em;color:#a1a1aa}
</style>
</head><body>
<div class="c">
<h1>Connected!</h1>
<p>Your device is connecting to the WiFi network.<br>It will appear in the Trellis app shortly.</p>
</div>
</body></html>
)rawliteral";

TrellisProvisioning::TrellisProvisioning(const char* deviceName)
  : _deviceName(deviceName), _server(nullptr), _configured(false) {}

bool TrellisProvisioning::hasCredentials() {
#if defined(ESP32)
  Preferences prefs;
  prefs.begin("trellis", true);
  bool has = prefs.isKey("ssid");
  prefs.end();
  return has;
#elif defined(ARDUINO_ARCH_RP2040)
  if (!LittleFS.begin()) return false;
  return LittleFS.exists("/trellis_wifi.txt");
#endif
  return false;
}

String TrellisProvisioning::getSSID() {
#if defined(ESP32)
  Preferences prefs;
  prefs.begin("trellis", true);
  String ssid = prefs.getString("ssid", "");
  prefs.end();
  return ssid;
#elif defined(ARDUINO_ARCH_RP2040)
  if (!LittleFS.begin()) return "";
  File f = LittleFS.open("/trellis_wifi.txt", "r");
  if (!f) return "";
  String ssid = f.readStringUntil('\n');
  ssid.trim();
  f.close();
  return ssid;
#endif
  return "";
}

String TrellisProvisioning::getPassword() {
#if defined(ESP32)
  Preferences prefs;
  prefs.begin("trellis", true);
  String pass = prefs.getString("pass", "");
  prefs.end();
  return pass;
#elif defined(ARDUINO_ARCH_RP2040)
  if (!LittleFS.begin()) return "";
  File f = LittleFS.open("/trellis_wifi.txt", "r");
  if (!f) return "";
  f.readStringUntil('\n'); // skip SSID
  String pass = f.readStringUntil('\n');
  pass.trim();
  f.close();
  return pass;
#endif
  return "";
}

void TrellisProvisioning::saveCredentials(const String& ssid, const String& password) {
#if defined(ESP32)
  Preferences prefs;
  prefs.begin("trellis", false);
  prefs.putString("ssid", ssid);
  prefs.putString("pass", password);
  prefs.end();
#elif defined(ARDUINO_ARCH_RP2040)
  LittleFS.begin();
  File f = LittleFS.open("/trellis_wifi.txt", "w");
  f.println(ssid);
  f.println(password);
  f.close();
#endif
  Serial.printf("[Trellis] Credentials saved for: %s\n", ssid.c_str());
}

void TrellisProvisioning::clearCredentials() {
#if defined(ESP32)
  Preferences prefs;
  prefs.begin("trellis", false);
  prefs.clear();
  prefs.end();
#elif defined(ARDUINO_ARCH_RP2040)
  LittleFS.begin();
  LittleFS.remove("/trellis_wifi.txt");
#endif
  Serial.println("[Trellis] Credentials cleared");
}

bool TrellisProvisioning::startProvisioningAP(uint16_t port) {
  // Create AP
  String apName = "Trellis-";
  apName += _deviceName;
  apName.replace(" ", "-");

  WiFi.mode(WIFI_AP);
  WiFi.softAP(apName.c_str());
  delay(500);

  Serial.printf("[Trellis] AP started: %s (IP: %s)\n",
    apName.c_str(), WiFi.softAPIP().toString().c_str());
  Serial.println("[Trellis] Open http://192.168.4.1 to configure WiFi");

  _server = new WebServer(port);
  _configured = false;

  _server->on("/", HTTP_GET, [this]() { handleRoot(); });
  _server->on("/configure", HTTP_POST, [this]() { handleConfigure(); });
  _server->on("/scan", HTTP_GET, [this]() { handleScan(); });

  // Captive portal: redirect all requests to setup page
  _server->onNotFound([this]() {
    _server->sendHeader("Location", "/");
    _server->send(302, "text/plain", "");
  });

  _server->begin();

  // Block until configured
  while (!_configured) {
    _server->handleClient();
    delay(10);
  }

  _server->stop();
  delete _server;
  _server = nullptr;

  WiFi.softAPdisconnect(true);
  WiFi.mode(WIFI_STA);

  // Save and return
  saveCredentials(_receivedSSID, _receivedPassword);
  return true;
}

bool TrellisProvisioning::autoConnect(unsigned long timeout_ms) {
  if (hasCredentials()) {
    String ssid = getSSID();
    String pass = getPassword();

    Serial.printf("[Trellis] Connecting to saved network: %s\n", ssid.c_str());
    WiFi.begin(ssid.c_str(), pass.c_str());

    unsigned long start = millis();
    while (WiFi.status() != WL_CONNECTED) {
      if (millis() - start > timeout_ms) {
        Serial.println("[Trellis] Saved network connection failed");
        WiFi.disconnect();
        break;
      }
      delay(250);
      Serial.print(".");
    }

    if (WiFi.status() == WL_CONNECTED) {
      Serial.printf("\n[Trellis] Connected! IP: %s\n", WiFi.localIP().toString().c_str());
      return true;
    }
  }

  // No credentials or connection failed — start provisioning
  Serial.println("[Trellis] Starting WiFi provisioning AP...");
  startProvisioningAP();

  // Now connect with the new credentials
  String ssid = getSSID();
  String pass = getPassword();

  WiFi.begin(ssid.c_str(), pass.c_str());
  unsigned long start = millis();
  while (WiFi.status() != WL_CONNECTED) {
    if (millis() - start > timeout_ms) {
      Serial.println("[Trellis] Connection failed after provisioning");
      return false;
    }
    delay(250);
    Serial.print(".");
  }

  Serial.printf("\n[Trellis] Connected! IP: %s\n", WiFi.localIP().toString().c_str());
  return true;
}

void TrellisProvisioning::handleRoot() {
  _server->send(200, "text/html", FPSTR(PROVISIONING_HTML));
}

void TrellisProvisioning::handleConfigure() {
  _receivedSSID = _server->arg("ssid");
  _receivedPassword = _server->arg("password");

  if (_receivedSSID.length() > 0) {
    _server->send(200, "text/html", FPSTR(PROVISIONING_SUCCESS));
    Serial.printf("[Trellis] Received credentials for: %s\n", _receivedSSID.c_str());
    delay(1000); // Let the response be sent
    _configured = true;
  } else {
    _server->sendHeader("Location", "/");
    _server->send(302, "text/plain", "");
  }
}

static String escapeJson(const String& input) {
  String out;
  out.reserve(input.length() + 10);
  for (unsigned int i = 0; i < input.length(); i++) {
    char c = input.charAt(i);
    if (c == '"') out += "\\\"";
    else if (c == '\\') out += "\\\\";
    else if (c == '<') out += "\\u003c";
    else if (c == '>') out += "\\u003e";
    else if (c == '\'') out += "\\u0027";
    else out += c;
  }
  return out;
}

void TrellisProvisioning::handleScan() {
  int n = WiFi.scanNetworks();
  String json = "[";
  for (int i = 0; i < n && i < 20; i++) {
    if (i > 0) json += ",";
    json += "{\"ssid\":\"" + escapeJson(WiFi.SSID(i)) + "\",\"rssi\":" + String(WiFi.RSSI(i)) + "}";
  }
  json += "]";
  WiFi.scanDelete();
  _server->send(200, "application/json", json);
}
