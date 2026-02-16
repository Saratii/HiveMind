using System.Collections.Generic;
using System.IO;
using UnityEngine;
using Newtonsoft.Json;

public class RoadGenerator : MonoBehaviour
{
    [Header("Input")]
    public string fileName = "city.json";

    [Header("Grid Settings")]
    [Tooltip("Meters per grid cell BEFORE worldScale is applied.")]
    public float cellSize = 5f;

    [Tooltip("Sampling step along each road segment = cellSize * sampleStepFactor (smaller = more accurate, more cells).")]
    public float sampleStepFactor = 0.5f;

    [Header("World Scale")]
    [Tooltip("Scales the entire map down in Unity space. 0.01 = 100x smaller.")]
    public float worldScale = 0.01f;  // <-- your default

    [Header("Road Shape")]
    [Tooltip("Road width in METERS (before worldScale). Try 8-14 for a 'normal' street.")]
    public float roadWidthMeters = 12f;

    [Header("Visuals")]
    [Tooltip("Road tile height (Y scale). This is thickness, not width.")]
    public float roadThickness = 0.2f;

    public float yOffset = 0f;
    public bool removeColliders = true;

    [Header("Road Color")]
    [Tooltip("Near-black asphalt by default.")]
    public Color roadColor = new Color(0.05f, 0.05f, 0.05f);
    [Range(0f, 1f)] public float glossiness = 0f;

    // JSON schema
    [System.Serializable]
    public class City { public Segment[] segments; }

    [System.Serializable]
    public class Segment
    {
        public int id;
        public float[][] pts; // each is [x,y] in meters
    }

    private readonly HashSet<Vector2Int> roadCells = new HashSet<Vector2Int>();

    void Start()
    {
        string path = Path.Combine(Application.streamingAssetsPath, fileName);
        if (!File.Exists(path))
        {
            Debug.LogError($"city.json not found at: {path}\nPlace it at Assets/StreamingAssets/{fileName}");
            return;
        }

        string json = File.ReadAllText(path);
        City city = JsonConvert.DeserializeObject<City>(json);

        if (city == null || city.segments == null || city.segments.Length == 0)
        {
            Debug.LogError("No segments found in city.json");
            return;
        }

        // How many grid cells around the centerline should we fill to create road width?
        int radiusCells = Mathf.Max(0, Mathf.CeilToInt((roadWidthMeters * 0.5f) / cellSize));

        // Sampling step along roads (in meters, before worldScale)
        float stepMeters = Mathf.Max(0.01f, cellSize * sampleStepFactor);

        foreach (var seg in city.segments)
        {
            if (seg.pts == null || seg.pts.Length < 2) continue;

            for (int i = 0; i < seg.pts.Length - 1; i++)
            {
                Vector2 a = new Vector2(seg.pts[i][0], seg.pts[i][1]);
                Vector2 b = new Vector2(seg.pts[i + 1][0], seg.pts[i + 1][1]);

                float len = Vector2.Distance(a, b);
                if (len < 1e-6f) continue;

                int samples = Mathf.CeilToInt(len / stepMeters);

                for (int s = 0; s <= samples; s++)
                {
                    float t = (samples == 0) ? 0f : (float)s / samples;
                    Vector2 p = Vector2.Lerp(a, b, t);

                    Vector2Int cell = WorldToCell(p);
                    AddRoadDisk(cell, radiusCells);
                }
            }
        }

        Debug.Log($"Road cells generated (with width): {roadCells.Count}");
        SpawnRoadTiles();
    }

    void SpawnRoadTiles()
    {
        // Pick a shader that matches your render pipeline to avoid pink materials.
        Shader shader =
            Shader.Find("Universal Render Pipeline/Lit") ??
            Shader.Find("HDRP/Lit") ??
            Shader.Find("Standard");

        if (shader == null)
        {
            Debug.LogError("Could not find a suitable shader (URP/HDRP/Standard). Roads will not render correctly.");
            return;
        }

        // Shared material for performance (don’t create one material per tile).
        Material sharedMat = new Material(shader);
        ApplyMaterialColorAndSmoothness(sharedMat);

        float tileXZ = cellSize * worldScale;

        foreach (var cell in roadCells)
        {
            Vector3 pos = CellToWorld(cell);

            GameObject road = GameObject.CreatePrimitive(PrimitiveType.Cube);
            road.name = $"Road_{cell.x}_{cell.y}";
            road.transform.SetParent(transform, false);
            road.transform.position = pos;
            road.transform.localScale = new Vector3(tileXZ, roadThickness, tileXZ);

            var renderer = road.GetComponent<Renderer>();
            if (renderer != null)
                renderer.sharedMaterial = sharedMat;

            if (removeColliders)
            {
                var col = road.GetComponent<Collider>();
                if (col) Destroy(col);
            }
        }
    }

    void ApplyMaterialColorAndSmoothness(Material mat)
    {
        // Color (works for Standard + URP Lit + HDRP Lit)
        if (mat.HasProperty("_BaseColor")) mat.SetColor("_BaseColor", roadColor); // URP/HDRP
        else if (mat.HasProperty("_Color")) mat.SetColor("_Color", roadColor);    // Built-in Standard
        else mat.color = roadColor;

        // Smoothness / Glossiness (property name varies by pipeline/shader)
        if (mat.HasProperty("_Smoothness")) mat.SetFloat("_Smoothness", glossiness);   // URP/HDRP often
        if (mat.HasProperty("_Glossiness")) mat.SetFloat("_Glossiness", glossiness);   // Built-in Standard
    }

    Vector2Int WorldToCell(Vector2 worldMeters)
    {
        int gx = Mathf.FloorToInt(worldMeters.x / cellSize);
        int gy = Mathf.FloorToInt(worldMeters.y / cellSize);
        return new Vector2Int(gx, gy);
    }

    Vector3 CellToWorld(Vector2Int cell)
    {
        float x = (cell.x + 0.5f) * cellSize * worldScale;
        float z = (cell.y + 0.5f) * cellSize * worldScale;
        return new Vector3(x, yOffset, z);
    }

    // Fill a disk of cells around the centerline to make roads wide.
    void AddRoadDisk(Vector2Int center, int radiusCells)
    {
        if (radiusCells <= 0)
        {
            roadCells.Add(center);
            return;
        }

        int r2 = radiusCells * radiusCells;
        for (int dx = -radiusCells; dx <= radiusCells; dx++)
        {
            for (int dy = -radiusCells; dy <= radiusCells; dy++)
            {
                if (dx * dx + dy * dy <= r2)
                    roadCells.Add(new Vector2Int(center.x + dx, center.y + dy));
            }
        }
    }
}
