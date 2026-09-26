import { create } from "zustand";

import type { CarsData, Positions, State } from "@/types/state.type";

// main store

type DataStore = {
	state: State | null;
	carsData: CarsData | null;
	positions: Positions | null;

	setState: (state: Partial<State> | null) => void;
	removeTopics: (topics: string[]) => void;
	setCarsData: (carsData: CarsData | null) => void;
	setPositions: (positions: Positions | null) => void;
};

export const useDataStore = create<DataStore>((set) => ({
	state: null,
	carsData: null,
	positions: null,

	setState: (partialState: Partial<State> | null) =>
		set((prev) => ({
			state: partialState
				? {
						...(prev.state ?? {}),
						// eslint-disable-next-line @typescript-eslint/no-unused-vars
						...Object.fromEntries(Object.entries(partialState).filter(([_, v]) => v !== undefined)),
					}
				: null,
		})),
	removeTopics: (topics: string[]) =>
		set((prev) => ({
			state: prev.state
				? (Object.fromEntries(Object.entries(prev.state).filter(([key]) => !topics.includes(key))) as State)
				: null,
		})),
	setCarsData: (carsData: CarsData | null) => set({ carsData }),
	setPositions: (positions: Positions | null) => set({ positions }),
}));
