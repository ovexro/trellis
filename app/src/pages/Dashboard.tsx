import { useEffect } from "react";
import { Radar } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";
import DeviceCard from "@/components/DeviceCard";

export default function Dashboard() {
  const { devices, scanning, scan } = useDeviceStore();

  useEffect(() => {
    scan();
  }, [scan]);

  if (devices.length === 0 && !scanning) {
    return (
      <div className="flex flex-col items-center justify-center h-full text-center">
        <Radar size={48} className="text-zinc-700 mb-4" />
        <h2 className="text-lg font-semibold text-zinc-300 mb-2">
          No devices found
        </h2>
        <p className="text-sm text-zinc-500 max-w-sm mb-6">
          Make sure your ESP32 or Pico W is running the Trellis library
          and connected to the same network.
        </p>
        <button
          onClick={scan}
          className="px-4 py-2 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm transition-colors"
        >
          Scan Network
        </button>
      </div>
    );
  }

  return (
    <div>
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {devices.map((device) => (
          <DeviceCard key={device.id} device={device} />
        ))}
      </div>
    </div>
  );
}
