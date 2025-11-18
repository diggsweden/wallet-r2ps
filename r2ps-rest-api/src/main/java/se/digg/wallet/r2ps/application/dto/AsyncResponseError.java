package se.digg.wallet.r2ps.application.dto;

public record AsyncResponseError(String message, int httpStatus) {
}
