"""Run pure JVM asset-update tests without generated Tauri/Android build files.

Uses the repository's Gradle wrapper, Kotlin version, and existing JVM test dependencies.
Set JAVA_HOME to a JDK supported by that wrapper (the Android Studio JBR works).
"""

import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    android = Path(__file__).resolve().parents[1] / "src-tauri/gen/android"
    wrapper = android / ("gradlew.bat" if os.name == "nt" else "gradlew")
    source = lambda kind: json.dumps((android / f"app/src/{kind}/java").as_posix()).replace("$", "\\$")
    with tempfile.TemporaryDirectory(prefix="agentshangout-client-tests-") as temporary:
        project = Path(temporary)
        (project / "settings.gradle.kts").write_text('rootProject.name = "client-policy-tests"\n')
        (project / "build.gradle.kts").write_text(f'''
plugins {{ kotlin("jvm") version "2.2.10" }}
repositories {{ mavenCentral() }}
dependencies {{
    testImplementation("junit:junit:4.13.2")
    implementation("org.json:json:20240303")
}}
sourceSets {{
    main {{ kotlin {{
        srcDir({source("main")})
        include("**/ClientUpdatePolicy.kt", "**/ClientBundleStore.kt", "**/OfflinePolicy.kt")
    }} }}
    test {{ kotlin {{
        srcDir({source("test")})
        include("**/ClientUpdatePolicyTest.kt", "**/ClientBundleStoreTest.kt", "**/OfflinePolicyTest.kt")
    }} }}
}}
''', encoding="utf-8")
        result = subprocess.run([str(wrapper), "-p", str(project), "test", "--console=plain"], cwd=android)
        if result.returncode:
            for report in project.glob("build/test-results/test/TEST-*.xml"):
                print(report.read_text(encoding="utf-8"))
        return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
