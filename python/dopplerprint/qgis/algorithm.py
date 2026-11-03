from qgis.core import (
    QgsCoordinateReferenceSystem,
    QgsProcessing,
    QgsProcessingAlgorithm,
    QgsProcessingContext,
    QgsProcessingException,
    QgsProcessingParameterDateTime,
    QgsProcessingParameterExtent,
    QgsProcessingParameterFile,
    QgsProcessingParameterFileDestination,
    QgsProcessingParameterNumber,
)

LAYERS = ("cells", "regions", "tracks", "candidates")


class LocateRecordingAlgorithm(QgsProcessingAlgorithm):
    AUDIO, ADSB, START, EXTENT, MAX_HOURS, OUTPUT = "AUDIO", "ADSB", "START", "EXTENT", "MAX_HOURS", "OUTPUT"

    def name(self):
        return "locate_recording"

    def displayName(self):  # noqa: N802 (QGIS API name)
        return "Locate recording"

    def shortHelpString(self):  # noqa: N802
        return ("Matches aircraft Doppler curves in an audio recording against historical ADS-B tracks and writes "
                "scored cells, ranked candidates, tracks and credible regions to a styled GeoPackage.")

    def createInstance(self):  # noqa: N802
        return LocateRecordingAlgorithm()

    def initAlgorithm(self, config=None):  # noqa: N802
        self.addParameter(QgsProcessingParameterFile(self.AUDIO, "Audio recording"))
        self.addParameter(QgsProcessingParameterFile(
            self.ADSB, "ADS-B store (Parquet file or directory)", behavior=QgsProcessingParameterFile.Folder))
        self.addParameter(QgsProcessingParameterDateTime(self.START, "Approximate recording start (UTC)", optional=True))
        self.addParameter(QgsProcessingParameterExtent(self.EXTENT, "Search extent", optional=True))
        self.addParameter(QgsProcessingParameterNumber(
            self.MAX_HOURS, "Time prior half-width (h)", QgsProcessingParameterNumber.Double, 1.0, minValue=0.0))
        self.addParameter(QgsProcessingParameterFileDestination(
            self.OUTPUT, "Result GeoPackage", "GeoPackage (*.gpkg)"))

    def processAlgorithm(self, parameters, context, feedback):  # noqa: N802
        try:
            import dopplerprint
        except ImportError as e:
            raise QgsProcessingException("dopplerprint is not installed in QGIS's Python (pip install dopplerprint)") from e
        search = getattr(dopplerprint, "search", None)
        if search is None:
            raise QgsProcessingException("this dopplerprint build has no search() API yet")

        out = self.parameterAsFileOutput(parameters, self.OUTPUT, context)
        start = self.parameterAsDateTime(parameters, self.START, context)
        extent = self.parameterAsExtent(parameters, self.EXTENT, context, QgsCoordinateReferenceSystem("EPSG:4326"))
        search(
            audio=self.parameterAsFile(parameters, self.AUDIO, context),
            adsb=self.parameterAsFile(parameters, self.ADSB, context),
            start_utc=start.toSecsSinceEpoch() if start.isValid() else None,
            extent=None if extent.isNull() else (extent.xMinimum(), extent.yMinimum(), extent.xMaximum(), extent.yMaximum()),
            max_hours=self.parameterAsDouble(parameters, self.MAX_HOURS, context),
            gpkg=out,
            progress=lambda f: feedback.setProgress(100.0 * f),
            cancelled=feedback.isCanceled,
        )
        for layer in LAYERS:
            uri = f"{out}|layername={layer}"
            context.addLayerToLoadOnCompletion(
                uri, QgsProcessingContext.LayerDetails(layer, context.project(), layer, QgsProcessing.TypeVectorAnyGeometry))
        return {self.OUTPUT: out}
