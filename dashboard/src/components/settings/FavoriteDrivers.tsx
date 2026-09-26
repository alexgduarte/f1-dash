"use client";

import { motion } from "motion/react";
import Image from "next/image";

import xIcon from "public/icons/xmark.svg";

import { useSettingsStore } from "@/stores/useSettingsStore";
import { useDataStore } from "@/stores/useDataStore";

import DriverTag from "@/components/driver/DriverTag";
import SelectMultiple from "@/components/ui/SelectMultiple";

export default function FavoriteDrivers() {
	// the settings page lives inside the dashboard layout, so the live driver list is already here
	const driverList = useDataStore((state) => state.state?.DriverList);
	const drivers = driverList ? Object.values(driverList).filter((driver) => !!driver?.RacingNumber) : null;

	const { favoriteDrivers, setFavoriteDrivers, removeFavoriteDriver } = useSettingsStore();

	return (
		<div className="flex flex-col gap-2">
			<div className="flex gap-2">
				{favoriteDrivers.map((driverNumber) => {
					const driver = drivers?.find((d) => d.RacingNumber === driverNumber);

					if (!driver) return null;

					return (
						<div key={driverNumber} className="flex items-center gap-1 rounded-xl border border-zinc-800 p-1">
							<DriverTag teamColor={driver.TeamColour} short={driver.Tla} />

							<motion.button
								whileHover={{ scale: 1.05 }}
								whileTap={{ scale: 0.95 }}
								onClick={() => removeFavoriteDriver(driverNumber)}
							>
								<Image src={xIcon} alt="x" width={30} />
							</motion.button>
						</div>
					);
				})}
			</div>

			<div className="w-80">
				<SelectMultiple
					placeholder="Select favorite drivers"
					options={drivers ? drivers.map((d) => ({ label: d.FullName, value: d.RacingNumber })) : []}
					selected={favoriteDrivers}
					setSelected={setFavoriteDrivers}
				/>
			</div>
		</div>
	);
}
