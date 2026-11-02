"""M2 end-to-end regression: an installed APK rotates while Android rotation is locked.

Only an emulator is accepted. No physical phone settings or account data are changed.
"""
import argparse
import re
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument('--serial', required=True)
args = parser.parse_args()
if not args.serial.startswith('emulator-'):
    parser.error('this sensor test only runs on an emulator')


def adb(*command):
    return subprocess.check_output(['adb', '-s', args.serial, *command], text=True).strip()


def await_rotation(landscape):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        state = adb('shell', 'dumpsys', 'window', 'displays')
        match = re.search(r'init=\d+x\d+.*?cur=(\d+)x(\d+)', state)
        if match and (int(match[1]) > int(match[2])) == landscape:
            return
        time.sleep(0.5)
    raise AssertionError(f'M2: app did not rotate to {"landscape" if landscape else "portrait"} with auto-rotate off')


def current_activity():
    state = adb('shell', 'dumpsys', 'activity', 'activities')
    match = re.search(r'topResumedActivity=ActivityRecord\{(\w+) [^\n]*com\.agentshangout\.android/', state)
    assert match, 'M2: the app must be in the foreground'
    return match[1]


original_rotation = adb('shell', 'settings', 'get', 'system', 'accelerometer_rotation')
original_sensor = re.search(r'acceleration = ([^\r\n]+)', adb('emu', 'sensor', 'get', 'acceleration'))[1]
try:
    adb('shell', 'settings', 'put', 'system', 'accelerometer_rotation', '0')
    adb('emu', 'sensor', 'set', 'acceleration', '0:9.8:0.5')
    adb('shell', 'am', 'start', '-n', 'com.agentshangout.android/.MainActivity')
    await_rotation(False)
    activity = current_activity()
    adb('emu', 'sensor', 'set', 'acceleration', '9.8:0:0.5')
    await_rotation(True)
    assert current_activity() == activity, 'M2: rotating must not recreate the activity'
    print('PASS M2: landscape follows the sensor with system auto-rotate off', flush=True)
    adb('emu', 'sensor', 'set', 'acceleration', '0:9.8:0.5')
    await_rotation(False)
    assert current_activity() == activity, 'M2: returning to portrait must keep the activity'
    assert adb('shell', 'settings', 'get', 'system', 'accelerometer_rotation') == '0'
    print('PASS M2: returns to portrait without changing the system preference', flush=True)
finally:
    adb('emu', 'sensor', 'set', 'acceleration', original_sensor)
    adb('shell', 'settings', 'put', 'system', 'accelerometer_rotation', original_rotation)
