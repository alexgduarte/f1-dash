import WeatherMap from "@/components/weather/WeatherMap";

export default function WeatherPage() {
	// calc height is a workaround, maybe think about refactoring sometime
	return (
		<div className="relative h-[calc(100%-142px)] w-full md:h-full">
			<WeatherMap />
		</div>
	);
}
