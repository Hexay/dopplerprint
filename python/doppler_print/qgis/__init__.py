"""QGIS plugin entry point: copy or symlink this directory into the QGIS plugins folder."""


def classFactory(iface):  # noqa: N802 (QGIS API name)
    from .plugin import DopplerPrintPlugin

    return DopplerPrintPlugin(iface)
