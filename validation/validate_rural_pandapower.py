#!/usr/bin/env python3
"""Independent rural-case comparison against pandapower runpp_3ph.

This script intentionally remains outside the regular Rust test suite. It needs
the optional dependency pinned in validation/requirements.txt and invokes the
CalcElec executable supplied on the command line.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any

import pandapower as pp

PHASES = ("A", "B", "C")


def complex_value(value: dict[str, float]) -> complex:
    return complex(value["r"], value["x"])


def phase_values(equipment: dict[str, Any], electrical: dict[str, Any]) -> tuple[list[float], list[float]]:
    p = [0.0, 0.0, 0.0]
    q = [0.0, 0.0, 0.0]
    connection = electrical["connection"]
    if connection["type"] == "phase_neutral":
        phase = PHASES.index(connection["phase"])
        p[phase] = equipment["p_w"] / 1e6
        q[phase] = equipment["q_var"] / 1e6
    elif connection["type"] == "three_phase_star" and connection["neutral"]:
        p = [equipment["p_w"] / 3e6] * 3
        q = [equipment["q_var"] / 3e6] * 3
    else:
        raise ValueError(f"unsupported validation injection: {connection}")
    return p, q


def symmetrical_line_impedances(electrical: dict[str, Any]) -> tuple[complex, complex, bool]:
    matrix = electrical["series_ohm_per_km"]
    z = [[complex_value(value) for value in row] for row in matrix]
    tolerance = 1e-12
    for phase in range(3):
        if abs(z[phase][phase] - z[0][0]) > tolerance:
            raise ValueError("pandapower comparison requires equal phase self impedances")
    mutual = z[0][1]
    for phase in range(3):
        for other in range(3):
            if phase != other and abs(z[phase][other] - mutual) > tolerance:
                raise ValueError("pandapower comparison requires equal phase mutual impedances")
    positive = z[0][0] - mutual
    zero = z[0][0] + 2 * mutual
    explicit_neutral = len(z) == 4
    if explicit_neutral:
        phase_neutral = z[0][3]
        if any(abs(z[phase][3] - phase_neutral) > tolerance for phase in range(3)):
            raise ValueError("pandapower projection requires equal phase-neutral couplings")
        if abs(z[3][3]) <= tolerance:
            raise ValueError("neutral self impedance cannot be zero for Kron reduction")
        # Pandapower has no explicit metallic neutral. Kron reduction is the
        # documented projection used only for the external comparison.
        zero -= 3 * phase_neutral * z[3][0] / z[3][3]
    return positive, zero, explicit_neutral


def build_pandapower(objects: dict[str, Any]) -> tuple[Any, dict[str, int], set[str], set[str]]:
    net = pp.create_empty_network(sn_mva=1.0, f_hz=50.0)
    buses: dict[str, int] = {}
    transformer_secondaries: set[str] = set()
    explicit_neutral_buses: set[str] = set()

    for object_id, obj in objects.items():
        equipment = obj["equipment"]
        if equipment["type"] == "bus" and obj["in_service"]:
            buses[object_id] = pp.create_bus(
                net,
                vn_kv=equipment["voltage_v"] / 1000.0,
                name=obj["name"],
                in_service=True,
            )

    for object_id, obj in objects.items():
        if not obj["in_service"]:
            continue
        equipment = obj["equipment"]
        electrical = obj.get("electrical")
        kind = equipment["type"]
        if kind == "source":
            # CalcElec currently imposes an ideal source in load flow and does
            # not use its short-circuit impedances. A very stiff pandapower
            # external grid reproduces that declared assumption.
            pp.create_ext_grid(
                net,
                buses[equipment["bus"]],
                vm_pu=equipment["voltage_pu"],
                name=obj["name"],
                s_sc_max_mva=1e9,
                s_sc_min_mva=1e9,
                rx_max=0.1,
                rx_min=0.1,
                r0x0_max=0.1,
                x0x_max=1.0,
            )
        elif kind == "line":
            positive, zero, explicit_neutral = symmetrical_line_impedances(electrical)
            if explicit_neutral:
                explicit_neutral_buses.update((equipment["from"], equipment["to"]))
            pp.create_line_from_parameters(
                net,
                buses[equipment["from"]],
                buses[equipment["to"]],
                length_km=equipment["length_m"] / 1000.0,
                r_ohm_per_km=positive.real,
                x_ohm_per_km=positive.imag,
                c_nf_per_km=0.0,
                max_i_ka=equipment["max_current_a"] / 1000.0,
                r0_ohm_per_km=zero.real,
                x0_ohm_per_km=zero.imag,
                c0_nf_per_km=0.0,
                name=obj["name"],
            )
        elif kind == "transformer":
            if electrical["from"]["connection"] != "delta" or electrical["to"]["connection"] != "star":
                raise ValueError("the rural comparison currently covers Dyn transformers")
            zero = complex_value(electrical["zero_sequence_ohm_referred_from"])
            zero_base = equipment["rated_va"] / electrical["from"]["rated_voltage_ll_v"] ** 2
            # CalcElec uses theta = -clock*30 degrees, normalized. Pandapower's
            # shift sign is opposite for the same from-HV/to-LV orientation.
            calc_angle = -(electrical["clock"] * 30.0)
            calc_angle = (calc_angle + 180.0) % 360.0 - 180.0
            pp.create_transformer_from_parameters(
                net,
                buses[equipment["from"]],
                buses[equipment["to"]],
                sn_mva=equipment["rated_va"] / 1e6,
                vn_hv_kv=electrical["from"]["rated_voltage_ll_v"] / 1000.0,
                vn_lv_kv=electrical["to"]["rated_voltage_ll_v"] / 1000.0,
                vkr_percent=100.0 * electrical["copper_loss_w"] / equipment["rated_va"],
                vk_percent=equipment["uk_percent"],
                pfe_kw=electrical["iron_loss_w"] / 1000.0,
                i0_percent=electrical["no_load_current_percent"],
                shift_degree=-calc_angle,
                vector_group="Dyn",
                vk0_percent=100.0 * abs(zero) * zero_base,
                vkr0_percent=100.0 * zero.real * zero_base,
                mag0_percent=1.0,
                mag0_rx=0.0,
                si0_hv_partial=0.9,
                name=obj["name"],
            )
            transformer_secondaries.add(equipment["to"])
        elif kind in ("load", "generator"):
            p, q = phase_values(equipment, electrical)
            creator = pp.create_asymmetric_load if kind == "load" else pp.create_asymmetric_sgen
            creator(
                net,
                buses[equipment["bus"]],
                p_a_mw=p[0],
                p_b_mw=p[1],
                p_c_mw=p[2],
                q_a_mvar=q[0],
                q_b_mvar=q[1],
                q_c_mvar=q[2],
                type="wye",
                name=obj["name"],
            )
        elif kind in ("bus",):
            continue
        else:
            raise ValueError(f"unsupported active object in rural comparison: {kind} ({object_id})")
    return net, buses, transformer_secondaries, explicit_neutral_buses


def angle_difference(left: float, right: float) -> float:
    return abs((left - right + 180.0) % 360.0 - 180.0)


def metric(name: str, calc: float, reference: float, tolerance: float, unit: str) -> dict[str, Any]:
    calc = float(calc)
    reference = float(reference)
    tolerance = float(tolerance)
    difference = abs(calc - reference)
    return {
        "name": name,
        "calcelec": calc,
        "pandapower": reference,
        "absolute_difference": difference,
        "tolerance": tolerance,
        "unit": unit,
        "passed": bool(difference <= tolerance),
    }


def compare(project_path: Path, executable: Path) -> dict[str, Any]:
    project = json.loads(project_path.read_text(encoding="utf-8"))
    if project["changes"]:
        raise ValueError("this validator expects a compact reference project without changes")
    objects = project["root"]["objects"]
    net, bus_index, transformer_secondaries, explicit_neutral_buses = build_pandapower(objects)
    pp.runpp_3ph(
        net,
        calculate_voltage_angles=True,
        max_iteration=100,
        tolerance_mva=1e-10,
        numba=False,
    )
    if not net.converged:
        raise RuntimeError("pandapower did not converge")

    completed = subprocess.run(
        [str(executable), "--unbalanced-load-flow", str(project_path)],
        check=True,
        capture_output=True,
        text=True,
    )
    study = json.loads(completed.stdout)
    result = study["result"]
    calc_buses = {entry["id"]: entry for entry in result["buses"]}

    max_hta_voltage = (0.0, "")
    max_secondary_voltage = (0.0, "")
    max_neutral_projection_voltage = (0.0, "")
    max_angle = (0.0, "")
    bus_differences = []
    for object_id, pp_bus in bus_index.items():
        calc_bus = calc_buses[object_id]
        pp_result = net.res_bus_3ph.loc[pp_bus]
        pp_voltages = [pp_result[f"vm_{phase.lower()}_pu"] for phase in PHASES]
        pp_angles = [pp_result[f"va_{phase.lower()}_degree"] for phase in PHASES]
        calc_voltages = [entry["pu"] for entry in calc_bus["phase_neutral"]]
        calc_angles = [entry["angle_deg"] for entry in calc_bus["phase_neutral"]]
        voltage_difference = max(abs(a - b) for a, b in zip(calc_voltages, pp_voltages))
        angle_error = max(angle_difference(a, b) for a, b in zip(calc_angles, pp_angles))
        label = objects[object_id]["name"]
        bus_differences.append(
            {
                "bus": label,
                "voltage_max_abs_pu": voltage_difference,
                "angle_max_abs_degree": angle_error,
            }
        )
        if object_id in explicit_neutral_buses:
            if voltage_difference > max_neutral_projection_voltage[0]:
                max_neutral_projection_voltage = (voltage_difference, label)
        elif object_id in transformer_secondaries:
            if voltage_difference > max_secondary_voltage[0]:
                max_secondary_voltage = (voltage_difference, label)
        elif voltage_difference > max_hta_voltage[0]:
            max_hta_voltage = (voltage_difference, label)
        if angle_error > max_angle[0]:
            max_angle = (angle_error, label)

    source_p = sum(float(net.res_ext_grid_3ph.iloc[0][f"p_{phase.lower()}_mw"]) for phase in PHASES) * 1e6
    source_q = sum(float(net.res_ext_grid_3ph.iloc[0][f"q_{phase.lower()}_mvar"]) for phase in PHASES) * 1e6
    line_loss = sum(float(net.res_line_3ph[f"pl_{phase.lower()}_mw"].sum()) for phase in PHASES) * 1e6
    transformer_loss = sum(float(net.res_trafo_3ph[f"pl_{phase.lower()}_mw"].sum()) for phase in PHASES) * 1e6

    metrics = [
        metric("source_active_power", result["source_p_w"], source_p, 30.0, "W"),
        metric("source_reactive_power", result["source_q_var"], source_q, 10.0, "var"),
        metric("line_active_losses", result["line_loss_w"], line_loss, 30.0, "W"),
        metric("transformer_active_losses", result["transformer_loss_w"], transformer_loss, 2.0, "W"),
        metric("HTA_voltage_maximum", max_hta_voltage[0], 0.0, 5e-5, "pu"),
        metric("transformer_secondary_voltage_maximum", max_secondary_voltage[0], 0.0, 3e-4, "pu"),
        metric("explicit_neutral_projection_voltage_maximum", max_neutral_projection_voltage[0], 0.0, 7e-3, "pu"),
        metric("phase_angle_maximum", max_angle[0], 0.0, 0.5, "degree"),
    ]
    return {
        "schema": "calcelec-independent-validation-1",
        "case": str(project_path),
        "engines": {
            "calcelec": study["solver_version"],
            "pandapower": pp.__version__,
            "pandapower_solver": "runpp_3ph",
        },
        "mapping": {
            "source": "stiff external grid matching CalcElec's ideal load-flow source",
            "ABC_lines": "positive and zero sequence derived from the primitive matrix",
            "ABCN_line": "Kron-reduced neutral projection; earth and neutral are not equivalent",
            "transformers": "Dyn with positive/zero-sequence test impedances",
        },
        "worst_buses": {
            "HTA_voltage": max_hta_voltage[1],
            "transformer_secondary_voltage": max_secondary_voltage[1],
            "explicit_neutral_projection_voltage": max_neutral_projection_voltage[1],
            "phase_angle": max_angle[1],
        },
        "metrics": metrics,
        "bus_differences": sorted(bus_differences, key=lambda value: value["bus"]),
        "passed": all(item["passed"] for item in metrics),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--project", type=Path, default=Path("examples/reference-rural-unbalanced.json"))
    parser.add_argument("--calcelec", type=Path, default=Path("target/debug/calcelec"))
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    try:
        report = compare(arguments.project, arguments.calcelec)
    except Exception as error:  # keep optional-tool failures concise for CI logs
        print(f"validation failed: {error}", file=sys.stderr)
        return 2
    rendered = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if arguments.output:
        arguments.output.write_text(rendered, encoding="utf-8")
    else:
        print(rendered, end="")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
