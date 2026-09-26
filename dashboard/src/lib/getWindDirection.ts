export const getWindDirection = (deg: number) => {
	const directions = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
	// each direction covers the 45° centred on it, so 350° is N rather than NW
	return directions[Math.round((((deg % 360) + 360) % 360) / 45) % 8];
};
