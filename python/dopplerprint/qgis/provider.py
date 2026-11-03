from qgis.core import QgsProcessingProvider

from .algorithm import LocateRecordingAlgorithm


class DopplerPrintProvider(QgsProcessingProvider):
    def id(self):
        return "dopplerprint"

    def name(self):
        return "dopplerprint"

    def longName(self):  # noqa: N802 (QGIS API name)
        return "dopplerprint: Doppler/ADS-B recording locator"

    def loadAlgorithms(self):  # noqa: N802
        self.addAlgorithm(LocateRecordingAlgorithm())
