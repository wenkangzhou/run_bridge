import os
from collections import namedtuple

# getting content root directory
current = os.path.dirname(os.path.realpath(__file__))
parent = os.path.dirname(current)

# Allow overriding via environment variable for release builds
# (macOS .app bundle resources are read-only)
OUTPUT_DIR = os.environ.get("RUN_BRIDGE_OUTPUT_DIR", os.path.join(parent, "activities"))
GPX_FOLDER = os.environ.get("RUN_BRIDGE_GPX_FOLDER", os.path.join(parent, "GPX_OUT"))
TCX_FOLDER = os.environ.get("RUN_BRIDGE_TCX_FOLDER", os.path.join(parent, "TCX_OUT"))
FIT_FOLDER = os.environ.get("RUN_BRIDGE_FIT_FOLDER", os.path.join(parent, "FIT_OUT"))
PNG_FOLDER = os.environ.get("RUN_BRIDGE_PNG_FOLDER", os.path.join(parent, "PNG_OUT"))
ENDOMONDO_FILE_DIR = os.environ.get("RUN_BRIDGE_ENDOMONDO_DIR", os.path.join(parent, "Workouts"))
FOLDER_DICT = {
    "gpx": GPX_FOLDER,
    "tcx": TCX_FOLDER,
    "fit": FIT_FOLDER,
}
SQL_FILE = os.environ.get("RUN_BRIDGE_SQL_FILE", os.path.join(parent, "run_page", "data.db"))
JSON_FILE = os.environ.get("RUN_BRIDGE_JSON_FILE", os.path.join(parent, "src", "static", "activities.json"))
SYNCED_FILE = os.environ.get("RUN_BRIDGE_SYNCED_FILE", os.path.join(parent, "imported.json"))


BASE_TIMEZONE = "Asia/Shanghai"
UTC_TIMEZONE = "UTC"

start_point = namedtuple("start_point", "lat lon")
run_map = namedtuple("polyline", "summary_polyline")
