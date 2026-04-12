"""
merge_glb.py
Merges all meshes in a GLB into a single mesh node, reducing per-car entity
count in Bevy from N-meshes down to 1 (per material).

Usage:
    python merge_glb.py input.glb output.glb

Requirements:
    pip install pygltflib numpy
"""

import sys
import struct
import numpy as np
from pygltflib import GLTF2, Mesh, Primitive, Node, Accessor, BufferView, Buffer
from pygltflib import FLOAT, UNSIGNED_INT, UNSIGNED_SHORT, ARRAY_BUFFER, ELEMENT_ARRAY_BUFFER

COMPONENT_TYPE_SIZE = {
    5120: 1, 5121: 1,  # BYTE, UNSIGNED_BYTE
    5122: 2, 5123: 2,  # SHORT, UNSIGNED_SHORT
    5124: 4, 5125: 4,  # INT, UNSIGNED_INT
    5126: 4,           # FLOAT
}

COMPONENT_TYPE_FORMAT = {
    5120: 'b', 5121: 'B',
    5122: 'h', 5123: 'H',
    5124: 'i', 5125: 'I',
    5126: 'f',
}

TYPE_NUM_COMPONENTS = {
    "SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4,
    "MAT2": 4, "MAT3": 9, "MAT4": 16,
}


def read_accessor(gltf, accessor_index):
    """Return a flat list of values for an accessor."""
    acc = gltf.accessors[accessor_index]
    bv = gltf.bufferViews[acc.bufferView]
    buf_data = gltf.binary_blob()

    comp_size = COMPONENT_TYPE_SIZE[acc.componentType]
    num_comp = TYPE_NUM_COMPONENTS[acc.type]
    fmt_char = COMPONENT_TYPE_FORMAT[acc.componentType]

    byte_offset = (bv.byteOffset or 0) + (acc.byteOffset or 0)
    stride = bv.byteStride or (comp_size * num_comp)

    values = []
    for i in range(acc.count):
        row_offset = byte_offset + i * stride
        row = []
        for c in range(num_comp):
            pos = row_offset + c * comp_size
            (v,) = struct.unpack_from(fmt_char, buf_data, pos)
            row.append(v)
        values.append(row if num_comp > 1 else row[0])
    return values, acc.type, acc.componentType


def node_local_matrix(node):
    """Return the 4x4 local transform matrix for a GLTF node as a numpy array."""
    if node.matrix is not None:
        # GLTF stores matrices column-major
        return np.array(node.matrix, dtype=np.float32).reshape(4, 4).T

    t = np.array(node.translation or [0, 0, 0], dtype=np.float32)
    r = np.array(node.rotation or [0, 0, 0, 1], dtype=np.float32)  # xyzw
    s = np.array(node.scale or [1, 1, 1], dtype=np.float32)

    # Scale
    S = np.diag([s[0], s[1], s[2], 1.0]).astype(np.float32)

    # Rotation from quaternion (xyzw)
    x, y, z, w = r
    R = np.array([
        [1 - 2*(y*y + z*z),     2*(x*y - w*z),     2*(x*z + w*y), 0],
        [    2*(x*y + w*z), 1 - 2*(x*x + z*z),     2*(y*z - w*x), 0],
        [    2*(x*z - w*y),     2*(y*z + w*x), 1 - 2*(x*x + y*y), 0],
        [                0,                 0,                   0, 1],
    ], dtype=np.float32)

    # Translation
    T = np.eye(4, dtype=np.float32)
    T[0, 3] = t[0]
    T[1, 3] = t[1]
    T[2, 3] = t[2]

    return T @ R @ S


def collect_primitives_by_material(gltf):
    """Walk every mesh/primitive and group by (material_index, primitive) with world transform."""
    # groups: material_index -> list of (primitive, world_matrix_4x4)
    groups = {}

    def walk_node(node_idx, parent_matrix):
        node = gltf.nodes[node_idx]
        local = node_local_matrix(node)
        world = parent_matrix @ local

        if node.mesh is not None:
            mesh = gltf.meshes[node.mesh]
            for prim in mesh.primitives:
                mat = prim.material
                groups.setdefault(mat, []).append((prim, world))

        for child in (node.children or []):
            walk_node(child, world)

    identity = np.eye(4, dtype=np.float32)
    for scene in (gltf.scenes or []):
        for root in (scene.nodes or []):
            walk_node(root, identity)

    return groups


def transform_positions(pos_arr, matrix):
    """Apply a 4x4 matrix to an (N,3) position array."""
    ones = np.ones((len(pos_arr), 1), dtype=np.float32)
    homogeneous = np.concatenate([pos_arr, ones], axis=1)  # (N, 4)
    transformed = (matrix @ homogeneous.T).T  # (N, 4)
    return transformed[:, :3]


def transform_normals(nor_arr, matrix):
    """Apply the inverse-transpose of the upper 3x3 to normals (N,3)."""
    m3 = matrix[:3, :3]
    inv_t = np.linalg.inv(m3).T.astype(np.float32)
    transformed = (inv_t @ nor_arr.T).T
    # Re-normalize
    norms = np.linalg.norm(transformed, axis=1, keepdims=True)
    norms = np.where(norms < 1e-8, 1.0, norms)
    return (transformed / norms).astype(np.float32)


def merge_primitives(gltf, prim_matrix_pairs):
    """
    Merge a list of (Primitive, world_matrix) tuples into combined numpy arrays.
    Bakes each node's world transform into the vertex data before merging.
    Returns (positions, normals, uvs, indices) as numpy arrays, or None for
    optional attributes that are absent in all primitives.
    """
    all_pos, all_nor, all_uv, all_idx = [], [], [], []
    vertex_offset = 0

    for prim, world_matrix in prim_matrix_pairs:
        attrs = prim.attributes

        # --- POSITION (required) ---
        pos_data, _, _ = read_accessor(gltf, attrs.POSITION)
        pos_arr = np.array(pos_data, dtype=np.float32)
        pos_arr = transform_positions(pos_arr, world_matrix)
        all_pos.append(pos_arr)

        # --- NORMAL (optional) ---
        if attrs.NORMAL is not None:
            nor_data, _, _ = read_accessor(gltf, attrs.NORMAL)
            all_nor.append(np.array(nor_data, dtype=np.float32))
        else:
            all_nor.append(None)

        # --- TEXCOORD_0 (optional) ---
        if attrs.TEXCOORD_0 is not None:
            uv_data, _, _ = read_accessor(gltf, attrs.TEXCOORD_0)
            all_uv.append(np.array(uv_data, dtype=np.float32))
        else:
            all_uv.append(None)

        # --- INDICES ---
        if prim.indices is not None:
            idx_data, _, _ = read_accessor(gltf, prim.indices)
            idx_arr = np.array(idx_data, dtype=np.uint32) + vertex_offset
        else:
            # Non-indexed: synthesize sequential indices
            count = len(pos_arr)
            idx_arr = np.arange(vertex_offset, vertex_offset + count, dtype=np.uint32)

        all_idx.append(idx_arr)
        vertex_offset += len(pos_arr)

    combined_pos = np.concatenate(all_pos, axis=0)
    combined_idx = np.concatenate(all_idx, axis=0)

    has_normals = all(n is not None for n in all_nor)
    combined_nor = np.concatenate(all_nor, axis=0) if has_normals else None

    has_uvs = all(u is not None for u in all_uv)
    combined_uv = np.concatenate(all_uv, axis=0) if has_uvs else None

    return combined_pos, combined_nor, combined_uv, combined_idx


def pack_float32(arr):
    return arr.astype(np.float32).tobytes()


def pack_uint32(arr):
    return arr.astype(np.uint32).tobytes()


def build_merged_glb(gltf, groups):
    """Build a new minimal GLTF2 with one mesh containing one primitive per material group."""
    from pygltflib import (
        GLTF2, Asset, Scene, Node, Mesh, Primitive, Accessor,
        BufferView, Buffer, Attributes,
    )

    new_gltf = GLTF2()
    new_gltf.asset = gltf.asset

    # Copy materials verbatim
    new_gltf.materials = list(gltf.materials or [])
    # Copy textures, images, samplers verbatim
    new_gltf.textures = list(gltf.textures or [])
    new_gltf.images = list(gltf.images or [])
    new_gltf.samplers = list(gltf.samplers or [])

    binary_chunks = []
    current_offset = 0

    def add_buffer_view(data, target):
        nonlocal current_offset
        binary_chunks.append(data)
        bv = BufferView()
        bv.buffer = 0
        bv.byteOffset = current_offset
        bv.byteLength = len(data)
        bv.target = target
        new_gltf.bufferViews.append(bv)
        current_offset += len(data)
        # Align to 4 bytes
        pad = (4 - (len(data) % 4)) % 4
        if pad:
            binary_chunks.append(b'\x00' * pad)
            current_offset += pad
        return len(new_gltf.bufferViews) - 1

    def add_accessor(bv_index, component_type, count, acc_type, min_val=None, max_val=None):
        acc = Accessor()
        acc.bufferView = bv_index
        acc.byteOffset = 0
        acc.componentType = component_type
        acc.count = count
        acc.type = acc_type
        if min_val is not None:
            acc.min = min_val
        if max_val is not None:
            acc.max = max_val
        new_gltf.accessors.append(acc)
        return len(new_gltf.accessors) - 1

    primitives = []

    for mat_index, prims in groups.items():
        print(f"  Merging {len(prims)} primitive(s) for material {mat_index} ...")
        pos, nor, uv, idx = merge_primitives(gltf, prims)

        # POSITION
        pos_bytes = pack_float32(pos)
        pos_bv = add_buffer_view(pos_bytes, ARRAY_BUFFER)
        pos_min = pos.min(axis=0).tolist()
        pos_max = pos.max(axis=0).tolist()
        pos_acc = add_accessor(pos_bv, FLOAT, len(pos), "VEC3", pos_min, pos_max)

        # NORMAL
        nor_acc = None
        if nor is not None:
            nor_bytes = pack_float32(nor)
            nor_bv = add_buffer_view(nor_bytes, ARRAY_BUFFER)
            nor_acc = add_accessor(nor_bv, FLOAT, len(nor), "VEC3")

        # TEXCOORD_0
        uv_acc = None
        if uv is not None:
            uv_bytes = pack_float32(uv)
            uv_bv = add_buffer_view(uv_bytes, ARRAY_BUFFER)
            uv_acc = add_accessor(uv_bv, FLOAT, len(uv), "VEC2")

        # INDICES
        idx_bytes = pack_uint32(idx)
        idx_bv = add_buffer_view(idx_bytes, ELEMENT_ARRAY_BUFFER)
        idx_acc = add_accessor(idx_bv, UNSIGNED_INT, len(idx), "SCALAR")

        attrs = Attributes()
        attrs.POSITION = pos_acc
        if nor_acc is not None:
            attrs.NORMAL = nor_acc
        if uv_acc is not None:
            attrs.TEXCOORD_0 = uv_acc

        prim = Primitive()
        prim.attributes = attrs
        prim.indices = idx_acc
        prim.material = mat_index
        primitives.append(prim)

    merged_mesh = Mesh()
    merged_mesh.name = "merged"
    merged_mesh.primitives = primitives
    new_gltf.meshes.append(merged_mesh)

    root_node = Node()
    root_node.mesh = 0
    new_gltf.nodes.append(root_node)

    scene = Scene()
    scene.nodes = [0]
    new_gltf.scenes.append(scene)
    new_gltf.scene = 0

    # Assemble binary buffer
    total_binary = b''.join(binary_chunks)

    # Re-attach any images that were embedded in the original binary blob
    # (textures stored as bufferViews pointing into the old blob)
    # We need to copy those image bufferViews into the new blob too.
    orig_blob = gltf.binary_blob()
    if orig_blob and new_gltf.images:
        orig_bvs = gltf.bufferViews or []
        for img in new_gltf.images:
            if img.bufferView is not None:
                orig_bv = orig_bvs[img.bufferView]
                img_bytes = orig_blob[orig_bv.byteOffset: orig_bv.byteOffset + orig_bv.byteLength]
                new_bv = BufferView()
                new_bv.buffer = 0
                new_bv.byteOffset = len(total_binary)
                new_bv.byteLength = len(img_bytes)
                # remap the image's bufferView index to the new one
                img.bufferView = len(new_gltf.bufferViews)
                new_gltf.bufferViews.append(new_bv)
                total_binary += img_bytes
                pad = (4 - (len(img_bytes) % 4)) % 4
                if pad:
                    total_binary += b'\x00' * pad

    buf = Buffer()
    buf.byteLength = len(total_binary)
    new_gltf.buffers.append(buf)
    new_gltf.set_binary_blob(total_binary)

    return new_gltf


def main():
    if len(sys.argv) < 3:
        print("Usage: python merge_glb.py input.glb output.glb")
        sys.exit(1)

    input_path = sys.argv[1]
    output_path = sys.argv[2]

    print(f"Loading {input_path} ...")
    gltf = GLTF2().load(input_path)

    # Count before
    total_before = sum(
        len(m.primitives) for m in (gltf.meshes or [])
    )
    node_count_before = len(gltf.nodes or [])
    print(f"Before: {node_count_before} nodes, {len(gltf.meshes or [])} meshes, {total_before} primitives")

    print("Grouping primitives by material ...")
    groups = collect_primitives_by_material(gltf)
    print(f"Found {len(groups)} material group(s) — output will have {len(groups)} primitive(s)")

    print("Merging ...")
    new_gltf = build_merged_glb(gltf, groups)

    print(f"Saving {output_path} ...")
    new_gltf.save(output_path)

    total_after = sum(len(m.primitives) for m in new_gltf.meshes)
    print(f"After:  1 node, 1 mesh, {total_after} primitive(s)")
    print("Done!")


if __name__ == "__main__":
    main()