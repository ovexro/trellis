/**
 * Bundled starter templates for the Get Started wizard.
 * These are shipped with the app (not stored in SQLite) and provide
 * one-click starting points for common IoT projects.
 */

export interface StarterCapability {
  id: string;
  type: "switch" | "sensor" | "slider" | "color" | "text";
  label: string;
  gpio: string;
  unit: string;
  min: string;
  max: string;
}

export interface StarterTemplate {
  id: string;
  name: string;
  description: string;
  icon: string; // lucide icon name hint for the UI
  board: "esp32" | "picow";
  capabilities: StarterCapability[];
}

export const starterTemplates: StarterTemplate[] = [
  {
    id: "blink",
    name: "Blink",
    description: "Toggle an LED from the dashboard. The simplest starting point.",
    icon: "lightbulb",
    board: "esp32",
    capabilities: [
      {
        id: "led",
        type: "switch",
        label: "LED",
        gpio: "2",
        unit: "",
        min: "",
        max: "",
      },
    ],
  },
  {
    id: "sensor-monitor",
    name: "Sensor Monitor",
    description: "Read an analog sensor and display its value with a live chart.",
    icon: "thermometer",
    board: "esp32",
    capabilities: [
      {
        id: "sensor_0",
        type: "sensor",
        label: "Temperature",
        gpio: "34",
        unit: "C",
        min: "",
        max: "",
      },
      {
        id: "status",
        type: "text",
        label: "Status",
        gpio: "",
        unit: "",
        min: "",
        max: "",
      },
    ],
  },
  {
    id: "smart-relay",
    name: "Smart Relay",
    description: "Control a relay module with on/off switch and optional timer slider.",
    icon: "zap",
    board: "esp32",
    capabilities: [
      {
        id: "relay",
        type: "switch",
        label: "Relay",
        gpio: "26",
        unit: "",
        min: "",
        max: "",
      },
      {
        id: "timer",
        type: "slider",
        label: "Auto-off (min)",
        gpio: "",
        unit: "",
        min: "0",
        max: "120",
      },
    ],
  },
  {
    id: "weather-station",
    name: "Weather Station",
    description: "Temperature, humidity, and pressure sensors with text status display.",
    icon: "cloud-sun",
    board: "esp32",
    capabilities: [
      {
        id: "temp",
        type: "sensor",
        label: "Temperature",
        gpio: "34",
        unit: "C",
        min: "",
        max: "",
      },
      {
        id: "humidity",
        type: "sensor",
        label: "Humidity",
        gpio: "35",
        unit: "%",
        min: "",
        max: "",
      },
      {
        id: "pressure",
        type: "sensor",
        label: "Pressure",
        gpio: "32",
        unit: "hPa",
        min: "",
        max: "",
      },
      {
        id: "conditions",
        type: "text",
        label: "Conditions",
        gpio: "",
        unit: "",
        min: "",
        max: "",
      },
    ],
  },
  {
    id: "greenhouse",
    name: "Greenhouse Controller",
    description: "Soil moisture sensor, water pump relay, and brightness slider for grow lights.",
    icon: "sprout",
    board: "esp32",
    capabilities: [
      {
        id: "soil",
        type: "sensor",
        label: "Soil Moisture",
        gpio: "34",
        unit: "%",
        min: "",
        max: "",
      },
      {
        id: "pump",
        type: "switch",
        label: "Water Pump",
        gpio: "26",
        unit: "",
        min: "",
        max: "",
      },
      {
        id: "light",
        type: "slider",
        label: "Grow Light",
        gpio: "25",
        unit: "",
        min: "0",
        max: "100",
      },
      {
        id: "temp",
        type: "sensor",
        label: "Temperature",
        gpio: "35",
        unit: "C",
        min: "",
        max: "",
      },
    ],
  },
];
