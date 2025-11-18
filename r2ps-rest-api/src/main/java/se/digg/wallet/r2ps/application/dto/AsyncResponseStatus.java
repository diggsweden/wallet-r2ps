package se.digg.wallet.r2ps.application.dto;

public enum AsyncResponseStatus {
  COMPLETE("complete"),
  PENDING("pending"),
  ERROR("error");

  private final String value;

  // Constructor
  AsyncResponseStatus(String value) {
    this.value = value;
  }

  // Getter method
  public String getValue() {
    return value;
  }

  // Optional: method to get enum from string
  public static AsyncResponseStatus fromString(String text) {
    for (AsyncResponseStatus status : AsyncResponseStatus.values()) {
      if (status.value.equalsIgnoreCase(text)) {
        return status;
      }
    }
    throw new IllegalArgumentException("No enum constant with value: " + text);
  }
}
