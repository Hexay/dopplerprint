from qgis.core import QgsProcessingProvider

from .algorithm import LocateRecordingAlgorithm


class DopplerPrintProvider(QgsProcessingProvider):
    def id(self):
        return "doppler_print"

    def name(self):
        return "doppler_print"

    def longName(self):  # noqa: N802 (QGIS API name)
        return "doppler_print: Doppler/ADS-B recording locator"

    def loadAlgorithms(self):  # noqa: N802
        self.addAlgorithm(LocateRecordingAlgorithm())
