using UnityEngine;

public class FlyCamera : MonoBehaviour
{
    [Header("Movement")]
    public float moveSpeed = 15f;
    public float fastSpeedMultiplier = 3f;

    [Header("Mouse Look")]
    public float mouseSensitivity = 2f;
    public bool lockCursor = true;

    float yaw;
    float pitch;

    void Start()
    {
        Vector3 rot = transform.eulerAngles;
        yaw = rot.y;
        pitch = rot.x;

        if (lockCursor)
        {
            Cursor.lockState = CursorLockMode.Locked;
            Cursor.visible = false;
        }
    }

    void Update()
    {
        // Mouse look
        yaw += Input.GetAxis("Mouse X") * mouseSensitivity * 100f * Time.deltaTime;
        pitch -= Input.GetAxis("Mouse Y") * mouseSensitivity * 100f * Time.deltaTime;
        pitch = Mathf.Clamp(pitch, -89f, 89f);

        transform.rotation = Quaternion.Euler(pitch, yaw, 0f);

        // Movement
        float speed = moveSpeed;
        if (Input.GetKey(KeyCode.LeftShift))
            speed *= fastSpeedMultiplier;

        Vector3 move = Vector3.zero;
        move += transform.forward * Input.GetAxis("Vertical");
        move += transform.right * Input.GetAxis("Horizontal");

        if (Input.GetKey(KeyCode.E)) move += Vector3.up;
        if (Input.GetKey(KeyCode.Q)) move += Vector3.down;

        transform.position += move * speed * Time.deltaTime;

        // Unlock cursor
        if (Input.GetKeyDown(KeyCode.Escape))
        {
            Cursor.lockState = CursorLockMode.None;
            Cursor.visible = true;
        }
    }
}
